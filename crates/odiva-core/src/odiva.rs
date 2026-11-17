//! The actor that thinks: one mailbox, one turn at a time, one owner of state.

use std::collections::HashMap;
use std::sync::Arc;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message};
use odiva_harness::{Decision, Turn};
use odiva_proto::{
    CoreHandle, CoreMessage, Presence, Resolution, Snapshot, UiMessage, WireMessage,
};
use tokio::sync::{mpsc, oneshot};

use crate::brief;
use crate::error::{Error, Result};
use crate::handoff::Handoff;
use crate::harness::Harness;
use crate::ids::Ids;
use crate::journal::Journal;
use crate::ledger::{self, Records};
use crate::line::{Line, LineId, Mode};
use crate::asking::Asking;
use crate::halt::Halt;
use crate::prompt::AFTER_SILENCE;
use crate::roster::Roster;
use crate::route::Route;
use crate::run::{self, Run};
use crate::say::{Said as Spoken, Say};
use crate::store::Store;
use crate::talk::Talk;
use crate::task::{Outcome, Report, Status, Task};
use crate::tools;
use crate::transcript::Transcript;
use crate::waiting::Waiting;
use crate::turn::TurnCancel;

/// The context window compaction is measured against.
const CONTEXT_WINDOW: u64 = 128_000;

/// Where in the window a line rotates.
const COMPACT_AT: u64 = 60;

/// How many messages survive a rotation.
const KEEP: usize = 6;

/// How long she may stay inside task lines before being sent back to normal.
const STARVE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// What a run reported, and what it needs answered.
pub enum Event {
    /// A harness stopped on a permission request.
    Asked {
        task: String,
        ask: odiva_harness::Ask,
        answer: oneshot::Sender<Decision>,
    },
    /// A run ended, one way or another.
    Ended {
        run: u64,
        task: String,
        outcome: std::result::Result<Turn, String>,
    },
}

/// The person said something.
pub struct Heard;

/// Start every queued task the slots allow.
pub struct Fill;

/// Answer whatever a worker is stopped on.
pub struct Permit {
    pub task: String,
    pub allow: bool,
}

/// Stop a task where it stands.
pub struct CancelTask {
    pub task: String,
}

/// The person agreed to stop work: this is the transition, not the request.
pub struct Pause;

/// A user task was accepted: leave idle, taking the note with you.
pub struct Accept {
    pub task: String,
    pub handoff: Handoff,
}

/// Go into one line, and come back if it says nothing.
pub struct Switch {
    pub line: LineId,
    pub timeout: std::time::Duration,
}

/// Step back to the normal line of the current state.
pub struct ToNormal;

/// The switch timeout fired.
pub struct Deadline {
    pub line: LineId,
    pub at: u64,
}

/// She has been inside task lines for too long.
pub struct Starve {
    pub at: u64,
}

pub struct Setup {
    pub store: Store,
    pub route: Arc<dyn Route>,
    pub prompt: String,
    pub out: mpsc::UnboundedSender<CoreMessage>,
    pub records: ActorRef<Records>,
    pub harness: Harness,
    pub journal: Journal,
    pub turn: TurnCancel,
    pub waiting: Waiting,
    pub asking: Asking,
    pub halt: Halt,
}

pub struct Odiva {
    store: Store,
    ids: Ids,
    route: Arc<dyn Route>,
    prompt: String,
    out: mpsc::UnboundedSender<CoreMessage>,
    records: ActorRef<Records>,
    harness: Harness,
    journal: Journal,
    mode: Mode,
    active: LineId,
    switch_at: u64,
    switch_for: std::time::Duration,
    starve_at: u64,
    waiting: Waiting,
    turn: TurnCancel,
    asking: Asking,
    halt: Halt,
    handoff: Option<Handoff>,
    runs: HashMap<String, Run>,
    permits: HashMap<String, oneshot::Sender<Decision>>,
    next_run: u64,
}

impl Actor for Odiva {
    type Args = Setup;
    type Error = Error;

    async fn on_start(args: Self::Args, _ctx: ActorRef<Self>) -> Result<Self> {
        let handoff = crate::state::State::load(&args.store).handoff;

        Ok(Self {
            store: args.store,
            ids: Ids::default(),
            route: args.route,
            prompt: args.prompt,
            out: args.out,
            records: args.records,
            harness: args.harness,
            journal: args.journal,
            mode: Mode::Idle,
            active: LineId::normal(Mode::Idle),
            switch_at: 0,
            switch_for: std::time::Duration::from_secs(600),
            starve_at: 0,
            waiting: args.waiting,
            turn: args.turn,
            asking: args.asking,
            halt: args.halt,
            handoff,
            runs: HashMap::new(),
            permits: HashMap::new(),
            next_run: 0,
        })
    }
}

impl Message<Heard> for Odiva {
    type Reply = ();

    async fn handle(&mut self, _heard: Heard, ctx: &mut Context<Self, Self::Reply>) {
        while let Some(text) = self.waiting.next() {
            self.journal.line(format!("对方：{text}"));

            let user = self.ids.theirs(&text);
            self.emit(CoreMessage::MessageAppended {
                message: user.clone(),
            });

            self.answer(&user.markdown, Some(&user), ctx).await;
        }
    }
}

impl Message<Fill> for Odiva {
    type Reply = ();

    async fn handle(&mut self, _fill: Fill, ctx: &mut Context<Self, Self::Reply>) {
        self.fill(ctx).await;
    }
}

impl Message<Permit> for Odiva {
    type Reply = ();

    async fn handle(&mut self, permit: Permit, _ctx: &mut Context<Self, Self::Reply>) {
        let Some(answer) = self.permits.remove(&permit.task) else {
            return;
        };

        let decision = if permit.allow {
            Decision::Allow
        } else {
            Decision::Refuse
        };

        let _ = answer.send(decision);
        self.journal.line(format!(
            "任务 {}：{}",
            permit.task,
            if permit.allow { "批了" } else { "拒了" }
        ));

        if let Some(mut task) = self.lookup(&permit.task).await {
            task.status = Status::OnHarness;
            self.update(task).await;
        }
    }
}

impl Message<Accept> for Odiva {
    type Reply = ();

    async fn handle(&mut self, accept: Accept, ctx: &mut Context<Self, Self::Reply>) {
        self.enter_busy(accept.task, accept.handoff).await;
        self.fill(ctx).await;
    }
}

impl Message<Switch> for Odiva {
    type Reply = ();

    async fn handle(&mut self, msg: Switch, ctx: &mut Context<Self, Self::Reply>) {
        let LineId::Task(id) = &msg.line else {
            return;
        };

        let Some(task) = self.lookup(id).await else {
            return;
        };

        self.active = msg.line.clone();
        self.switch_at += 1;
        self.starve_at += 1;
        self.switch_for = msg.timeout;

        let at = self.switch_at;
        let line = msg.line.clone();
        let me = ctx.actor_ref().clone();
        let timeout = msg.timeout;

        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let _ = me.tell(Deadline { line, at }).await;
        });

        let at = self.starve_at;
        let me = ctx.actor_ref().clone();

        tokio::spawn(async move {
            tokio::time::sleep(STARVE).await;
            let _ = me.tell(Starve { at }).await;
        });

        let text = format!(
            "（你切到了 {} 的上下文）\n\n{}\n\n要说什么就现在说；它没有动静就回 normal。",
            task.id,
            brief::reading(&task)
        );

        self.answer(&text, None, ctx).await;
    }
}

impl Message<ToNormal> for Odiva {
    type Reply = ();

    async fn handle(&mut self, _msg: ToNormal, _ctx: &mut Context<Self, Self::Reply>) {
        self.leave_task("你回了 normal").await;
    }
}

impl Message<Deadline> for Odiva {
    type Reply = ();

    async fn handle(&mut self, msg: Deadline, _ctx: &mut Context<Self, Self::Reply>) {
        if self.active != msg.line || self.switch_at != msg.at {
            return;
        }

        self.leave_task("它一直没有动静").await;
    }
}

impl Message<Starve> for Odiva {
    type Reply = ();

    async fn handle(&mut self, msg: Starve, _ctx: &mut Context<Self, Self::Reply>) {
        if self.starve_at != msg.at || !matches!(self.active, LineId::Task(_)) {
            return;
        }

        self.leave_task("在里面待太久了").await;
    }
}

impl Message<Pause> for Odiva {
    type Reply = ();

    async fn handle(&mut self, _pause: Pause, _ctx: &mut Context<Self, Self::Reply>) {
        self.pause().await;
    }
}

impl Message<CancelTask> for Odiva {
    type Reply = ();

    async fn handle(&mut self, cancel: CancelTask, ctx: &mut Context<Self, Self::Reply>) {
        if let Some(run) = self.runs.remove(&cancel.task) {
            run.cancel.cancel();
        }

        self.permits.remove(&cancel.task);

        if let Some(mut task) = self.lookup(&cancel.task).await {
            task.status = Status::Cancelled;
            self.update(task).await;
        }

        self.fill(ctx).await;
    }
}

impl Message<Event> for Odiva {
    type Reply = ();

    async fn handle(&mut self, event: Event, ctx: &mut Context<Self, Self::Reply>) {
        match event {
            Event::Asked { task, ask, answer } => {
                self.journal
                    .line(format!("任务 {task}：等你批准 {}", ask.title));

                if let Some(mut record) = self.lookup(&task).await {
                    record.status = Status::OnPermission(ask.title.clone());
                    self.update(record).await;
                }

                self.permits.insert(task.clone(), answer);
                self.notice(&task, ctx).await;
            }
            Event::Ended {
                run,
                task,
                outcome,
            } => {
                if self.runs.get(&task).map(|running| running.id) != Some(run) {
                    return;
                }

                self.runs.remove(&task);
                self.permits.remove(&task);
                self.settle(&task, outcome).await;
                self.fill(ctx).await;
                self.notice(&task, ctx).await;
            }
        }
    }
}

impl Odiva {
    /// Something happened outside the conversation: it enters her context and
    /// nothing else, so the record stays a record of what was said.
    async fn notice(&mut self, id: &str, ctx: &mut Context<Self, ()>) {
        let Some(task) = self.lookup(id).await else {
            return;
        };

        self.journal.line(format!("任务 {}：{}", task.id, task.line()));

        if let Status::Failed(reason) = &task.status {
            self.emit(CoreMessage::CoreFailed {
                note: format!("{} 失败了：{reason}", task.id),
            });
        }

        if let LineId::Task(active) = self.active.clone()
            && active != task.id
        {
            if let Err(error) = self
                .note_for_normal(&format!("〔{}：{}〕", task.id, task.line()))
                .await
            {
                self.journal.line(format!("写不了 normal 线：{error}"));
            }

            return;
        }

        if self.active == LineId::task(task.id.clone()) {
            self.watch_again(ctx);
        }

        let text = format!(
            "（后台的变化，不是对方说的）\n\n{}\n\n要不要说、说什么，你决定。",
            brief::reading(&task)
        );

        self.answer(&text, None, ctx).await;
    }

    /// A harness reaction is the reset the switch timeout is measured from.
    fn watch_again(&mut self, ctx: &mut Context<Self, ()>) {
        self.switch_at += 1;

        let at = self.switch_at;
        let line = self.active.clone();
        let me = ctx.actor_ref().clone();
        let timeout = self.switch_for;

        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let _ = me.tell(Deadline { line, at }).await;
        });
    }

    /// Back to the dispatcher of the current state.
    async fn leave_task(&mut self, why: &str) {
        let LineId::Task(id) = self.active.clone() else {
            return;
        };

        self.active = LineId::normal(self.mode);
        self.switch_at += 1;
        self.starve_at += 1;

        if let Err(error) = self
            .note_for_normal(&format!("〔回到 normal：{id}，{why}〕"))
            .await
        {
            self.journal.line(format!("写不了 normal 线：{error}"));
        }
    }

    async fn note_for_normal(&self, text: &str) -> Result<()> {
        let mut line = Line::open(
            &self.store,
            LineId::normal(self.mode),
            &self.prompt,
        )?;

        line.push_note(text)
    }

    async fn answer(
        &mut self,
        text: &str,
        user: Option<&WireMessage>,
        ctx: &mut Context<Self, ()>,
    ) {
        self.presence(Presence::Typing);

        let outcome = self.turn(text, user, ctx).await;

        self.presence(Presence::Online);

        if let Err(error) = outcome {
            let note = format!("这一轮没跑完：{error}");

            self.journal.line(format!("失败：{error}"));
            self.emit(CoreMessage::CoreFailed { note: note.clone() });
            eprintln!("odiva: {note}");
        }
    }

    async fn turn(
        &mut self,
        text: &str,
        user: Option<&WireMessage>,
        ctx: &mut Context<Self, ()>,
    ) -> Result<()> {
        let spoken = Spoken::default();
        let id = self.active.clone();

        let position = match id {
            LineId::Task(_) => tools::Position::Task,
            LineId::Normal(_) => tools::Position::Normal,
        };

        let roster = self.roster(ctx, &spoken, &id, self.mode, position);

        let mut talk = Talk::open(crate::talk::Activation {
            store: &self.store,
            id,
            route: &*self.route,
            prompt: &self.prompt,
            roster,
            said: spoken,
            seed: None,
            halt: self.halt.clone(),
        })?;

        if let Some(user) = user {
            talk.note(user)?;
        }

        let cancel = self.turn.begin();
        talk.run_until(text, &cancel).await?;
        self.turn.end();

        if !talk.spoke() && talk.failure().is_none() {
            let cancel = self.turn.begin();
            talk.run_until(AFTER_SILENCE, &cancel).await?;
            self.turn.end();
        }

        let spoke = talk.spoke();
        talk.flush()?;

        if let Some(reason) = talk.failure() {
            return Err(Error::Brain(reason));
        }

        match (&talk.monologue(), spoke) {
            (Some(words), false) => self
                .journal
                .line(format!("她没说话；模型的原话：{words}")),
            (None, false) => {
                self.journal.line("她没说话；模型也没有回话");
                return Err(Error::Brain(
                    "模型没有回话（没有 key，或者 provider 报错）".to_owned(),
                ));
            }
            _ => {}
        }

        if talk.used() * 100 >= CONTEXT_WINDOW * COMPACT_AT {
            talk.compact(KEEP)?;
        }

        Ok(())
    }

    /// A user task was accepted: the mode is no longer a guess.
    async fn enter_busy(&mut self, _task: String, handoff: Handoff) {
        self.mode = Mode::Busy;
        self.active = LineId::normal(Mode::Busy);
        self.switch_at += 1;
        self.starve_at += 1;
        self.handoff = Some(handoff.clone());

        let state = crate::state::State {
            handoff: Some(handoff.clone()),
        };

        if let Err(error) = state.save(&self.store) {
            self.journal.line(format!("写不了 state.json：{error}"));
            self.emit(CoreMessage::CoreFailed {
                note: format!("写不了 state.json：{error}"),
            });
        }

        if let Err(error) = self.open_busy(&handoff).await {
            self.journal.line(format!("进入忙线：{error}"));
            self.emit(CoreMessage::CoreFailed {
                note: format!("进入忙线：{error}"),
            });
        }
    }

    /// The busy line starts with the note and the work that is now on the table.
    async fn open_busy(&self, handoff: &Handoff) -> Result<()> {
        let mut note = handoff.render();

        let all = self.records.ask(ledger::Unfinished).await.unwrap_or_default();

        if !all.is_empty() {
            note.push_str("\n手上的活：\n");

            for task in all {
                note.push_str(&format!("- {}\n", task.line()));
            }
        }

        let mut line = Line::open(&self.store, LineId::normal(Mode::Busy), &self.prompt)?;
        line.push_note(note)
    }

    /// The pause the person agreed to: stop everything, and go back to idle.
    async fn pause(&mut self) {
        for (_, run) in self.runs.drain() {
            run.cancel.cancel();
        }

        let unfinished = self
            .records
            .ask(ledger::Unfinished)
            .await
            .unwrap_or_default();

        for mut task in unfinished {
            task.status = Status::Paused;
            self.update(task).await;
        }

        self.mode = Mode::Idle;
        self.active = LineId::normal(Mode::Idle);
        self.switch_at += 1;
        self.starve_at += 1;
        self.journal.line("暂停：回到空闲");

        if let Err(error) = self.leave_busy().await {
            self.journal.line(format!("离开忙线：{error}"));
            self.emit(CoreMessage::CoreFailed {
                note: format!("离开忙线：{error}"),
            });
        }

        self.handoff = None;

        if let Err(error) = crate::state::State::default().save(&self.store) {
            self.journal.line(format!("写不了 state.json：{error}"));
        }
    }

    /// What the idle line is told when the work stops: the same facts a restart
    /// would have to reconstruct.
    async fn leave_busy(&self) -> Result<()> {
        let all = self.records.ask(ledger::Everything).await.unwrap_or_default();

        let mut note = String::from("【回到空闲】\n");

        if let Some(handoff) = &self.handoff {
            note.push_str(&handoff.render());
            note.push('\n');
        }

        for task in all.iter().filter(|task| task.status == Status::Paused) {
            note.push_str(&format!("- 暂停：{}\n", task.line()));
        }

        for task in all
            .iter()
            .filter(|task| matches!(task.status, Status::Done | Status::Cancelled))
        {
            note.push_str(&format!("- 收工：{}\n", task.line()));
        }

        let mut line = Line::open(&self.store, LineId::normal(Mode::Idle), &self.prompt)?;
        line.push_note(note)
    }

    /// Starts queued tasks until every slot is busy.
    async fn fill(&mut self, ctx: &mut Context<Self, ()>) {
        loop {
            if self.runs.len() >= brief::SLOTS {
                return;
            }

            let unfinished = self
                .records
                .ask(ledger::Unfinished)
                .await
                .unwrap_or_default();

            let Some(mut next) = unfinished
                .into_iter()
                .find(|task| task.status == Status::OnSlot)
            else {
                return;
            };

            if let Err(error) = std::fs::create_dir_all(&next.cwd) {
                next.status = Status::Failed(format!("{}: {error}", next.cwd.display()));
                self.update(next).await;
                continue;
            }

            self.next_run += 1;
            let run = run::start(self.next_run, &next, &self.harness, ctx.actor_ref().clone());
            self.runs.insert(next.id.clone(), run);

            next.status = Status::OnHarness;
            self.update(next).await;
        }
    }

    async fn settle(&mut self, id: &str, outcome: std::result::Result<Turn, String>) {
        let Some(mut task) = self.lookup(id).await else {
            return;
        };

        match outcome {
            Ok(turn) => {
                task.said = turn.said.clone();
                task.report = Report::parse(&turn.said);
                task.status = match task.report.as_ref().map(|report| &report.status) {
                    Some(Outcome::Need) => Status::OnInstruction,
                    Some(Outcome::Blocked) => Status::Failed(
                        task.report
                            .as_ref()
                            .and_then(|report| report.question.clone())
                            .unwrap_or_else(|| "卡住了".to_owned()),
                    ),
                    Some(Outcome::Done) | None => Status::Done,
                };
            }
            Err(error) => task.status = Status::Failed(error),
        }

        self.update(task).await;
    }

    async fn lookup(&self, id: &str) -> Option<Task> {
        self.records
            .ask(ledger::Lookup { id: id.to_owned() })
            .await
            .ok()
            .flatten()
    }

    async fn update(&self, task: Task) {
        let _ = self.records.tell(ledger::Update(task)).await;
    }

    fn roster(
        &self,
        ctx: &mut Context<Self, ()>,
        spoken: &Spoken,
        line: &LineId,
        mode: Mode,
        position: tools::Position,
    ) -> Roster {
        let mut tools: Vec<Box<dyn yoagent::types::AgentTool>> = vec![Box::new(Say::new(
            self.out.clone(),
            self.ids.clone(),
            spoken.clone(),
        ))];

        tools.extend(tools::roster(
            self.records.clone(),
            ctx.actor_ref().clone(),
            &self.store,
            line,
            mode,
            position,
        ));

        if mode == Mode::Busy {
            tools.push(Box::new(crate::idle::RequestIdle::new(
                self.out.clone(),
                self.asking.clone(),
                self.halt.clone(),
                ctx.actor_ref().clone(),
            )));
        }

        Roster::of(tools).sourcing(Arc::new(self.waiting.clone()))
    }

    fn emit(&self, message: CoreMessage) {
        let _ = self.out.send(message);
    }

    fn presence(&self, presence: Presence) {
        self.emit(CoreMessage::PresenceChanged { presence });
    }
}

/// The window's end of the wire.
#[derive(Debug, Clone)]
pub struct Handle {
    out: mpsc::UnboundedSender<UiMessage>,
}

impl CoreHandle for Handle {
    fn send(&self, message: UiMessage) {
        let _ = self.out.send(message);
    }
}

/// Starts the core and answers with the window's two ends of the wire.
pub fn start(
    store: Store,
    route: Arc<dyn Route>,
    prompt: impl Into<String>,
    harness: Harness,
) -> (Handle, mpsc::UnboundedReceiver<CoreMessage>) {
    use kameo::actor::Spawn;

    let (out, messages) = mpsc::unbounded_channel();
    let journal = Journal::new(store.root().join("odiva.log"));

    let snapshot = match Transcript::new(store.transcript()).read() {
        Ok(messages) => Snapshot {
            messages,
            presence: Presence::Online,
        },
        Err(error) => {
            let _ = out.send(CoreMessage::CoreFailed {
                note: format!("读不了窗口的记录：{error}"),
            });

            Snapshot::default()
        }
    };

    let _ = out.send(CoreMessage::CoreAttachedSession { snapshot });

    let records = Records::spawn(ledger::Args {
        dir: store.tasks(),
        out: out.clone(),
        journal: journal.clone(),
    });

    let turn = TurnCancel::default();
    let waiting = Waiting::default();
    let asking = Asking::default();
    let halt = Halt::default();
    let pump_out = out.clone();

    let odiva = Odiva::spawn(Setup {
        store: store.clone(),
        route,
        prompt: prompt.into(),
        out,
        records,
        harness,
        journal,
        turn: turn.clone(),
        waiting: waiting.clone(),
        asking: asking.clone(),
        halt,
    });

    let (wire, mut inbox) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(message) = inbox.recv().await {
            match message {
                UiMessage::UserSubmittedText { text } => {
                    waiting.offer(text);
                    let _ = odiva.tell(Heard).await;
                }
                UiMessage::AnswerQuestion(answer) => {
                    if asking.answer(answer) {
                        let _ = pump_out.send(CoreMessage::QuestionClosed {
                            resolution: Resolution::Answered,
                        });
                    }
                }
                UiMessage::UserRequestedStop => {
                    if asking.withdraw() {
                        let _ = pump_out.send(CoreMessage::QuestionClosed {
                            resolution: Resolution::Withdrawn,
                        });
                    }

                    turn.cancel();
                }
            }
        }
    });

    (Handle { out: wire }, messages)
}
