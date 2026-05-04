// SPDX-License-Identifier: MPL-2.0

use crate::fl;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, about::About, qr_code::ErrorCorrection};

const INPUT_ID: fn() -> widget::Id = || widget::Id::new("main-input");

pub struct AppModel {
    core: cosmic::Core,
    input: String,
    qr_data: Option<widget::qr_code::Data>,
    ec_level: ErrorCorrection,
    show_about: bool,
    about: About,
}

#[derive(Debug, Clone)]
pub enum Message {
    InputChanged(String),
    ErrorCorrectionChanged(ErrorCorrection),
    SaveQrPng,
    SaveQrSvg,
    CopyQr,
    ToggleAbout,
    OpenUrl(String),
    Noop,
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "dev.lhdjung.QrNew";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, _flags: ()) -> (Self, Task<cosmic::Action<Message>>) {
        let about = About::default()
            .name(fl!("app-title"))
            .author(fl!("app-description"))
            .comments(concat!("Version ", env!("CARGO_PKG_VERSION")))
            .icon(widget::icon::from_name(Self::APP_ID))
            .links([("Github repository", env!("CARGO_PKG_REPOSITORY"))]);

        let mut app = AppModel {
            core,
            input: String::new(),
            qr_data: None,
            ec_level: ErrorCorrection::Medium,
            show_about: false,
            about,
        };

        let cmd = Task::batch([app.update_title(), widget::text_input::focus(INPUT_ID())]);
        (app, cmd)
    }

    fn header_end(&self) -> Vec<Element<'_, Message>> {
        vec![
            widget::button::icon(widget::icon::from_name("help-about-symbolic"))
                .on_press(Message::ToggleAbout)
                .into(),
        ]
    }

    fn context_drawer(&self) -> Option<cosmic::app::context_drawer::ContextDrawer<'_, Message>> {
        if !self.show_about {
            return None;
        }

        Some(cosmic::app::context_drawer::about(
            &self.about,
            |url| Message::OpenUrl(url.to_owned()),
            Message::ToggleAbout,
        ))
    }

    fn view(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();
        let space_l = spacing.space_l;
        let space_m = spacing.space_m;
        let space_s = spacing.space_s;

        let input = widget::text_input(fl!("input-placeholder"), &self.input)
            .on_input(Message::InputChanged)
            .width(Length::Fixed(400.0))
            .id(INPUT_ID());

        let ec_label = widget::tooltip(
            widget::text(fl!("ec-label")),
            widget::container(
                widget::text(fl!("ec-tooltip"))
                    .size(13)
                    .width(Length::Fixed(260.0)),
            ),
            widget::tooltip::Position::Bottom,
        );

        let ec_row = widget::row::with_children(vec![
            ec_label.into(),
            ec_button(fl!("ec-low"), ErrorCorrection::Low, self.ec_level),
            ec_button(fl!("ec-medium"), ErrorCorrection::Medium, self.ec_level),
            ec_button(fl!("ec-quartile"), ErrorCorrection::Quartile, self.ec_level),
            ec_button(fl!("ec-high"), ErrorCorrection::High, self.ec_level),
        ])
        .spacing(space_s)
        .align_y(Alignment::Center);

        let qr_area: Element<_> = if let Some(data) = &self.qr_data {
            let action_row = widget::row::with_children(vec![
                widget::button::standard(fl!("save-png"))
                    .on_press(Message::SaveQrPng)
                    .into(),
                widget::button::standard(fl!("save-svg"))
                    .on_press(Message::SaveQrSvg)
                    .into(),
                widget::button::standard(fl!("copy"))
                    .on_press(Message::CopyQr)
                    .into(),
            ])
            .spacing(space_s);

            widget::column::with_children(vec![
                widget::container(widget::qr_code(data).cell_size(8))
                    .padding(space_m)
                    .into(),
                action_row.into(),
            ])
            .align_x(Alignment::Center)
            .spacing(space_s)
            .into()
        } else {
            widget::container(widget::text(fl!("qr-placeholder")).size(14))
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(200.0))
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .into()
        };

        let content = widget::column::with_children(vec![
            widget::text::title2(fl!("app-title")).into(),
            input.into(),
            ec_row.into(),
            qr_area,
        ])
        .align_x(Alignment::Center)
        .spacing(space_l);

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }

    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::InputChanged(text) => {
                self.input = text;
                self.regenerate_qr();
            }

            Message::ErrorCorrectionChanged(level) => {
                self.ec_level = level;
                self.regenerate_qr();
            }

            Message::SaveQrPng => {
                let input = self.input.clone();
                let ec: qrcode::EcLevel = self.ec_level.into();
                return Task::perform(
                    async move {
                        let Some(handle) = rfd::AsyncFileDialog::new()
                            .add_filter("PNG Image", &["png"])
                            .set_file_name("qrcode.png")
                            .save_file()
                            .await
                        else {
                            return;
                        };
                        let Ok(code) =
                            qrcode::QrCode::with_error_correction_level(input.as_bytes(), ec)
                        else {
                            return;
                        };
                        let img = code
                            .render::<image::Luma<u8>>()
                            .quiet_zone(true)
                            .module_dimensions(10, 10)
                            .build();
                        let _ = img.save(handle.path());
                    },
                    |_| cosmic::Action::App(Message::Noop),
                );
            }

            Message::SaveQrSvg => {
                let input = self.input.clone();
                let ec: qrcode::EcLevel = self.ec_level.into();
                return Task::perform(
                    async move {
                        let Some(handle) = rfd::AsyncFileDialog::new()
                            .add_filter("SVG Image", &["svg"])
                            .set_file_name("qrcode.svg")
                            .save_file()
                            .await
                        else {
                            return;
                        };
                        let Ok(code) =
                            qrcode::QrCode::with_error_correction_level(input.as_bytes(), ec)
                        else {
                            return;
                        };
                        let svg = code
                            .render::<qrcode::render::svg::Color>()
                            .quiet_zone(true)
                            .build();
                        let _ = std::fs::write(handle.path(), svg);
                    },
                    |_| cosmic::Action::App(Message::Noop),
                );
            }

            Message::CopyQr => {
                let input = self.input.clone();
                let ec: qrcode::EcLevel = self.ec_level.into();
                return Task::perform(
                    async move {
                        let Ok(code) =
                            qrcode::QrCode::with_error_correction_level(input.as_bytes(), ec)
                        else {
                            return;
                        };
                        let img = code
                            .render::<image::Luma<u8>>()
                            .quiet_zone(true)
                            .module_dimensions(10, 10)
                            .build();
                        let width = img.width() as usize;
                        let height = img.height() as usize;
                        let rgba: Vec<u8> = img
                            .pixels()
                            .flat_map(|p| [p.0[0], p.0[0], p.0[0], 255u8])
                            .collect();
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            let _ = cb.set_image(arboard::ImageData {
                                width,
                                height,
                                bytes: rgba.into(),
                            });
                        }
                    },
                    |_| cosmic::Action::App(Message::Noop),
                );
            }

            Message::ToggleAbout => {
                self.show_about = !self.show_about;
                self.set_show_context(self.show_about);
            }

            Message::OpenUrl(url) => {
                let _ = open::that(url);
            }

            Message::Noop => {}
        }
        Task::none()
    }
}

impl AppModel {
    fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        if self.core.main_window_id().is_some() {
            self.set_window_title(fl!("app-title"))
        } else {
            Task::none()
        }
    }

    fn regenerate_qr(&mut self) {
        self.qr_data = if self.input.is_empty() {
            None
        } else {
            widget::qr_code::Data::with_error_correction(self.input.as_bytes(), self.ec_level).ok()
        };
    }
}

fn ec_button<'a>(
    label: String,
    level: ErrorCorrection,
    current: ErrorCorrection,
) -> Element<'a, Message> {
    let button = if level == current {
        widget::button::suggested(label)
    } else {
        widget::button::standard(label)
    };

    button
        .on_press(Message::ErrorCorrectionChanged(level))
        .into()
}
