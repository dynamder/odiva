//! A harness for tests: it speaks ACP v1 on stdio and follows a canned script.
//!
//! `ODIVA_FAKE_SCRIPT` picks the script: `chat` says two things and finishes,
//! `slow` takes its time about it, `permission` asks for something first, `need`
//! asks for words with a report envelope, `crash` dies before the handshake.
//!
//! Handlers run on the connection's event loop, so none of them waits for the
//! peer: a handler either answers, or registers a callback that answers.

use std::env;

use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest,
    PromptResponse, RequestPermissionOutcome, RequestPermissionRequest, SessionNotification,
    SessionUpdate, StopReason, TextContent, ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Agent, Client, Responder, Result, Stdio, V1};

const SESSION: &str = "fake-session";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Chat,
    Slow,
    Permission,
    Need,
    Crash,
}

impl Script {
    fn from_env() -> Self {
        match env::var("ODIVA_FAKE_SCRIPT").as_deref() {
            Ok("permission") => Self::Permission,
            Ok("need") => Self::Need,
            Ok("slow") => Self::Slow,
            Ok("crash") => Self::Crash,
            _ => Self::Chat,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let script = Script::from_env();

    if script == Script::Crash {
        std::process::exit(1);
    }

    Agent
        .v1()
        .name("fake-harness")
        .on_receive_request(
            async |initialize: InitializeRequest, responder, _connection| {
                responder.respond(
                    InitializeResponse::new(initialize.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async |_new_session: NewSessionRequest, responder, _connection| {
                responder.respond(NewSessionResponse::new(SESSION))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_prompt: PromptRequest, responder, connection| {
                answer(script, responder, connection).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(Stdio::new(), async |connection| {
            connection.incoming_closed().await;
            Ok(())
        })
        .await
}

async fn answer(
    script: Script,
    responder: Responder<PromptResponse, V1>,
    connection: ConnectionTo<Client>,
) -> Result<()> {
    if script == Script::Slow {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    }

    if script == Script::Need {
        say(
            &connection,
            "要动 `migrate_v1.sh` 得先确认一件事。\n\n```report\n\
             {\"status\": \"need\", \"question\": \"这个脚本还有别处引用，删掉吗？\"}\n```",
        )?;

        return responder.respond(PromptResponse::new(StopReason::EndTurn));
    }

    if script != Script::Permission {
        say(&connection, "README 改完了。")?;
        say(&connection, "日志那边还有个发现。")?;

        return responder.respond(PromptResponse::new(StopReason::EndTurn));
    }

    let request = RequestPermissionRequest::new(
        SESSION,
        ToolCallUpdate::new(
            ToolCallId::new("call-1"),
            ToolCallUpdateFields::new().title("删除 migrate_v1.sh"),
        ),
        vec![
            PermissionOption::new("allow-once", "允许", PermissionOptionKind::AllowOnce),
            PermissionOption::new("reject-once", "不要", PermissionOptionKind::RejectOnce),
        ],
    );

    let told = connection.clone();

    connection
        .send_request(request)
        .on_receiving_result(async move |answer| {
            say(&told, &outcome(answer?.outcome))?;
            responder.respond(PromptResponse::new(StopReason::EndTurn))
        })
}

fn outcome(outcome: RequestPermissionOutcome) -> String {
    match outcome {
        RequestPermissionOutcome::Selected(selected) => format!("denied:{}", selected.option_id),
        RequestPermissionOutcome::Cancelled => "denied:cancelled".to_owned(),
        _ => "denied:unrecognized".to_owned(),
    }
}

fn say(connection: &ConnectionTo<Client>, text: &str) -> Result<()> {
    connection.send_notification(SessionNotification::new(
        SESSION,
        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new(text),
        ))),
    ))
}
