use odiva::App;

fn main() -> iced::Result {
    iced::application(App::empty, App::update, App::view)
        .theme(App::theme)
        .run()
}
