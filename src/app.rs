// SPDX-License-Identifier: MPL-2.0

use crate::fl;
use cosmic::app::context_drawer;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, about::About, menu};
use qrcode::EcLevel;
use std::collections::HashMap;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

pub struct AppModel {
    core: cosmic::Core,
    context_page: ContextPage,
    about: About,
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    input: String,
    qr_data: Option<widget::qr_code::Data>,
    ec_level: EcLevel,
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchUrl(String),
    ToggleContextPage(ContextPage),
    InputChanged(String),
    EcLevelChanged(EcLevel),
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

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .links([(fl!("repository"), REPOSITORY)])
            .license(env!("CARGO_PKG_LICENSE"));

        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            about,
            key_binds: HashMap::new(),
            input: String::new(),
            qr_data: None,
            ec_level: EcLevel::M,
        };

        let command = app.update_title();
        (app, command)
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            menu::root(fl!("view")).apply(Element::from),
            menu::items(
                &self.key_binds,
                vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
            ),
        )]);

        vec![menu_bar.into()]
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let spacing = cosmic::theme::spacing();
        let space_l = spacing.space_l;
        let space_m = spacing.space_m;
        let space_s = spacing.space_s;

        let input = widget::text_input(fl!("input-placeholder"), &self.input)
            .on_input(Message::InputChanged)
            .width(Length::Fixed(400.0));

        let ec_row = widget::row::with_children(vec![
            widget::text(fl!("ec-label")).into(),
            ec_button(fl!("ec-low"), EcLevel::L, self.ec_level),
            ec_button(fl!("ec-medium"), EcLevel::M, self.ec_level),
            ec_button(fl!("ec-quartile"), EcLevel::Q, self.ec_level),
            ec_button(fl!("ec-high"), EcLevel::H, self.ec_level),
        ])
        .spacing(space_s)
        .align_y(Alignment::Center);

        let qr_area: Element<_> = if let Some(data) = &self.qr_data {
            widget::container(widget::qr_code(data).cell_size(8))
                .padding(space_m)
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

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::InputChanged(text) => {
                self.input = text;
                self.regenerate_qr();
            }

            Message::EcLevelChanged(level) => {
                self.ec_level = level;
                self.regenerate_qr();
            }

            Message::ToggleContextPage(page) => {
                if self.context_page == page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = page;
                    self.core.window.show_context = true;
                }
            }

            Message::LaunchUrl(url) => {
                let _ = open::that_detached(&url);
            }
        }
        Task::none()
    }
}

impl AppModel {
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let window_title = fl!("app-title");
        if self.core.main_window_id().is_some() {
            self.set_window_title(window_title)
        } else {
            Task::none()
        }
    }

    fn regenerate_qr(&mut self) {
        if self.input.is_empty() {
            self.qr_data = None;
            return;
        }
        self.qr_data = widget::qr_code::Data::new(self.input.as_bytes())
            .ok()
            .map(Some)
            .unwrap_or(None);
    }
}

fn ec_button<'a>(
    label: String,
    level: EcLevel,
    current: EcLevel,
) -> Element<'a, Message> {
    if level == current {
        widget::button::suggested(label)
            .on_press(Message::EcLevelChanged(level))
            .into()
    } else {
        widget::button::standard(label)
            .on_press(Message::EcLevelChanged(level))
            .into()
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
