// SPDX-License-Identifier: MPL-2.0

use crate::fl;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, qr_code::ErrorCorrection};

pub struct AppModel {
    core: cosmic::Core,
    input: String,
    qr_data: Option<widget::qr_code::Data>,
    ec_level: ErrorCorrection,
}

#[derive(Debug, Clone)]
pub enum Message {
    InputChanged(String),
    ErrorCorrectionChanged(ErrorCorrection),
    SaveQr,
    CopyQr,
    Noop,
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "dev.lukasjung.QrNew";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, _flags: ()) -> (Self, Task<cosmic::Action<Message>>) {
        let mut app = AppModel {
            core,
            input: String::new(),
            qr_data: None,
            ec_level: ErrorCorrection::Medium,
        };
        let cmd = app.update_title();
        (app, cmd)
    }

    fn view(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();
        let space_l = spacing.space_l;
        let space_m = spacing.space_m;
        let space_s = spacing.space_s;

        let input = widget::text_input(fl!("input-placeholder"), &self.input)
            .on_input(Message::InputChanged)
            .width(Length::Fixed(400.0));

        let ec_row = widget::row::with_children(vec![
            widget::text(fl!("ec-label")).into(),
            ec_button(fl!("ec-low"), ErrorCorrection::Low, self.ec_level),
            ec_button(fl!("ec-medium"), ErrorCorrection::Medium, self.ec_level),
            ec_button(fl!("ec-quartile"), ErrorCorrection::Quartile, self.ec_level),
            ec_button(fl!("ec-high"), ErrorCorrection::High, self.ec_level),
        ])
        .spacing(space_s)
        .align_y(Alignment::Center);

        let qr_area: Element<_> = if let Some(data) = &self.qr_data {
            let action_row = widget::row::with_children(vec![
                widget::button::standard(fl!("save"))
                    .on_press(Message::SaveQr)
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

            Message::SaveQr => {
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
                        let Ok(code) = qrcode::QrCode::with_error_correction_level(
                            input.as_bytes(),
                            ec,
                        ) else {
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

            Message::CopyQr => {
                let input = self.input.clone();
                let ec: qrcode::EcLevel = self.ec_level.into();
                return Task::perform(
                    async move {
                        let Ok(code) = qrcode::QrCode::with_error_correction_level(
                            input.as_bytes(),
                            ec,
                        ) else {
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
    if level == current {
        widget::button::suggested(label)
            .on_press(Message::ErrorCorrectionChanged(level))
            .into()
    } else {
        widget::button::standard(label)
            .on_press(Message::ErrorCorrectionChanged(level))
            .into()
    }
}
