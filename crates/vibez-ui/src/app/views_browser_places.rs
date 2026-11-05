//! Sample-browser places tree rendering.
//! Split from views_browser.rs; inherent methods on [`super::App`].

use iced::widget::{button, column, container, row, text, Space};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::icons;
use crate::message::Message;
use crate::state::SampleBrowserMode;
use crate::theme as th;

use super::views_browser_style::*;
use super::*;

const TREE_ICON_SIZE: f32 = 12.0;
const PLACE_ICON_SIZE: f32 = 13.0;
const PLACE_ROW_H: f32 = 26.0;
const PLACE_LABEL_SIZE: f32 = 11.5;
/// A disclosure chevron's width, kept by leaves too so icons line up.
const TOGGLE_W: f32 = 14.0;

fn tree_toggle(expanded: bool, message: Message) -> Element<'static, Message> {
    button(
        icons::icon(if expanded {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        })
        .size(TREE_ICON_SIZE)
        .color(th::text_muted()),
    )
    .on_press(message)
    .padding(0)
    .width(Length::Fixed(TOGGLE_W))
    .style(browser_utility_action_style)
    .into()
}

fn catalog_state_marker(state: &str) -> Option<Element<'static, Message>> {
    let (icon, color) = match state {
        "INDEXING" | "UPDATING" => (icons::REFRESH_CW, th::text_muted()),
        "STALE" | "WATCH ERR" | "WARN" => (icons::TRIANGLE_ALERT, th::danger()),
        _ => return None,
    };
    Some(icons::icon(icon).size(TREE_ICON_SIZE).color(color).into())
}

/// One row of the places list: `indent` in, a disclosure chevron when it
/// has children, its icon and name, a pill while `selected`, and anything
/// trailing at its end. `lit` lights the icon of the place being browsed.
struct PlaceRow<'a> {
    indent: f32,
    toggle: Option<Element<'a, Message>>,
    icon: char,
    label: String,
    selected: bool,
    lit: bool,
    on_press: Message,
    trailing: Vec<Element<'a, Message>>,
}

impl<'a> PlaceRow<'a> {
    fn new(icon: char, label: impl Into<String>, on_press: Message) -> Self {
        Self {
            indent: 0.0,
            toggle: None,
            icon,
            label: label.into(),
            selected: false,
            lit: false,
            on_press,
            trailing: Vec::new(),
        }
    }

    fn view(self) -> Element<'a, Message> {
        let selected = self.selected;
        let icon_color = if selected || self.lit {
            th::accent()
        } else {
            th::text_dim()
        };
        let mut content = row![
            icons::icon(self.icon)
                .size(PLACE_ICON_SIZE)
                .color(icon_color),
            text(self.label)
                .size(PLACE_LABEL_SIZE)
                .font(iced::Font {
                    weight: if selected {
                        iced::font::Weight::Semibold
                    } else {
                        iced::font::Weight::Medium
                    },
                    ..iced::Font::DEFAULT
                })
                .color(th::text())
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(7)
        .align_y(iced::Alignment::Center);
        for trailing in self.trailing {
            content = content.push(trailing);
        }
        let pill = button(container(content).center_y(PLACE_ROW_H))
            .on_press(self.on_press)
            .padding([0, 7])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| browser_place_button_style(selected, status));
        row![
            Space::with_width(self.indent),
            self.toggle
                .unwrap_or_else(|| Space::with_width(TOGGLE_W).into()),
            pill
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
        .into()
    }
}

/// A quiet row at the foot of the places list, such as Add root.
fn place_action<'a>(
    icon: char,
    label: &'static str,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(
        row![
            icons::icon(icon)
                .size(PLACE_ICON_SIZE)
                .color(th::text_muted()),
            text(label).size(PLACE_LABEL_SIZE).color(th::text_dim())
        ]
        .spacing(7)
        .align_y(iced::Alignment::Center),
    )
    .on_press_maybe(message)
    .padding(iced::Padding {
        top: 5.0,
        right: 7.0,
        bottom: 5.0,
        left: 7.0 + TOGGLE_W + 2.0,
    })
    .width(Length::Fill)
    .style(|_theme: &Theme, status| browser_place_button_style(false, status))
    .into()
}

impl App {
    pub(super) fn view_browser_places(&self) -> Element<'_, Message> {
        let local_active = self.state.browser.mode == SampleBrowserMode::Local;
        let remote_active = self.state.browser.mode == SampleBrowserMode::Remote;

        let mut places = column![
            browser_caption("PLACES"),
            PlaceRow {
                lit: local_active,
                ..PlaceRow::new(
                    icons::HARD_DRIVE,
                    "Local",
                    Message::Browser(BrowserMsg::SetSampleBrowserMode(SampleBrowserMode::Local)),
                )
            }
            .view(),
            PlaceRow {
                indent: BROWSER_TREE_INDENT_STEP,
                selected: local_active && self.state.browser.current_folder.is_none(),
                ..PlaceRow::new(
                    icons::LAYERS,
                    "All Roots",
                    Message::Browser(BrowserMsg::SelectLocalFolder(None)),
                )
            }
            .view(),
        ]
        .spacing(1);

        let mut local_tree_rows = Vec::new();
        for root in &self.state.browser.roots {
            let active = local_active
                && self
                    .state
                    .browser
                    .current_folder
                    .as_ref()
                    .is_some_and(|selected| selected == root);
            let expanded = self.state.browser.expanded_local_folders.contains(root);
            let has_children = self
                .state
                .browser
                .folders
                .iter()
                .any(|folder| folder.root_path == *root && folder.path.parent() == Some(root));
            let label = root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string());
            let remove = button(icons::icon(icons::X).size(9).color(th::text_muted()))
                .on_press(Message::Browser(BrowserMsg::RemoveSampleLibraryRoot(
                    root.clone(),
                )))
                .padding([2, 2])
                .style(browser_utility_action_style);
            let mut trailing: Vec<Element<'_, Message>> = Vec::new();
            if let Some(marker) = catalog_state_marker(self.state.browser.root_catalog_label(root))
            {
                trailing.push(marker);
            }
            trailing.push(remove.into());
            local_tree_rows.push(
                PlaceRow {
                    indent: BROWSER_TREE_INDENT_STEP,
                    toggle: has_children.then(|| {
                        tree_toggle(
                            expanded,
                            Message::Browser(BrowserMsg::ToggleLocalFolder(root.clone())),
                        )
                    }),
                    selected: active,
                    trailing,
                    ..PlaceRow::new(
                        icons::FOLDER,
                        label,
                        Message::Browser(BrowserMsg::SelectLocalFolder(Some(root.clone()))),
                    )
                }
                .view(),
            );
            if expanded {
                self.render_local_places_tree(root, root, 2, &mut local_tree_rows);
            }
        }
        for local_row in local_tree_rows {
            places = places.push(local_row);
        }

        let remote = &self.state.browser.remote;
        places = places.push(
            PlaceRow {
                toggle: Some(tree_toggle(
                    remote.place_expanded,
                    Message::Browser(BrowserMsg::ToggleRemotePlace),
                )),
                lit: remote_active,
                ..PlaceRow::new(
                    icons::CLOUD,
                    "Remote",
                    Message::Browser(BrowserMsg::SetSampleBrowserMode(SampleBrowserMode::Remote)),
                )
            }
            .view(),
        );
        if remote.place_expanded {
            places = places.push(
                PlaceRow {
                    indent: REMOTE_CONNECTION_INDENT,
                    toggle: Some(tree_toggle(
                        remote.connection_expanded,
                        Message::Browser(BrowserMsg::ToggleRemoteConnection),
                    )),
                    selected: remote_active && remote.current_path.is_empty(),
                    trailing: vec![text(remote.catalog_state.label())
                        .size(8)
                        .color(th::text_muted())
                        .into()],
                    ..PlaceRow::new(
                        icons::CLOUD,
                        remote.catalog.connection_name.as_str(),
                        Message::Browser(BrowserMsg::SelectRemoteFolder(String::new())),
                    )
                }
                .view(),
            );
            if remote.connection_expanded {
                let mut remote_rows = Vec::new();
                self.render_remote_places_tree("", 0, &mut remote_rows);
                for remote_row in remote_rows {
                    places = places.push(remote_row);
                }
            }
        }

        let rescan = (!self.state.browser.roots.is_empty() && !self.state.browser.scan_in_progress)
            .then_some(Message::RescanSampleLibrary);
        places = places.push(
            column![
                place_action(icons::PLUS, "Add root", Some(Message::AddSampleLibraryRoot)),
                place_action(icons::REFRESH_CW, "Rescan", rescan),
            ]
            .spacing(1)
            .padding(iced::Padding {
                top: 8.0,
                ..Default::default()
            }),
        );

        container(crate::widgets::quiet_scroll::vertical(
            container(places.padding(iced::Padding {
                top: 0.0,
                right: 8.0,
                bottom: 8.0,
                left: 0.0,
            }))
            .width(Length::Fill),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn render_local_places_tree<'a>(
        &'a self,
        root: &Path,
        parent: &Path,
        depth: usize,
        rows: &mut Vec<Element<'a, Message>>,
    ) {
        let mut children: Vec<_> = self
            .state
            .browser
            .folders
            .iter()
            .filter(|folder| folder.root_path == root && folder.path.parent() == Some(parent))
            .collect();
        children.sort_by_key(|folder| folder.name.to_lowercase());

        for folder in children {
            let expanded = self
                .state
                .browser
                .expanded_local_folders
                .contains(&folder.path);
            let active = self.state.browser.mode == SampleBrowserMode::Local
                && self.state.browser.current_folder.as_ref() == Some(&folder.path);
            let has_children =
                self.state.browser.folders.iter().any(|child| {
                    child.root_path == root && child.path.parent() == Some(&folder.path)
                });
            rows.push(
                PlaceRow {
                    indent: (depth as f32).min(BROWSER_TREE_MAX_DEPTH) * BROWSER_TREE_INDENT_STEP,
                    toggle: has_children.then(|| {
                        tree_toggle(
                            expanded,
                            Message::Browser(BrowserMsg::ToggleLocalFolder(folder.path.clone())),
                        )
                    }),
                    selected: active,
                    ..PlaceRow::new(
                        icons::FOLDER,
                        folder.name.clone(),
                        Message::Browser(BrowserMsg::SelectLocalFolder(Some(folder.path.clone()))),
                    )
                }
                .view(),
            );
            if expanded {
                self.render_local_places_tree(root, &folder.path, depth + 1, rows);
            }
        }
    }

    fn render_remote_places_tree<'a>(
        &'a self,
        parent: &str,
        depth: usize,
        rows: &mut Vec<Element<'a, Message>>,
    ) {
        let remote = &self.state.browser.remote;
        for &index in remote.catalog_child_indices(parent) {
            let folder = &remote.catalog.entries[index];
            if !folder.is_folder {
                continue;
            }
            let expanded = self
                .state
                .browser
                .remote
                .expanded
                .contains(&folder.provider_item_id);
            let active = self.state.browser.mode == SampleBrowserMode::Remote
                && self.state.browser.remote.current_path == folder.provider_item_id;
            let has_children = remote
                .catalog_child_indices(&folder.provider_item_id)
                .iter()
                .any(|&child| remote.catalog.entries[child].is_folder);
            rows.push(
                PlaceRow {
                    indent: remote_places_indent(depth),
                    toggle: has_children.then(|| {
                        tree_toggle(
                            expanded,
                            Message::Browser(BrowserMsg::ToggleRemoteFolder(
                                folder.provider_item_id.clone(),
                            )),
                        )
                    }),
                    selected: active,
                    ..PlaceRow::new(
                        icons::FOLDER,
                        folder.name.clone(),
                        Message::Browser(BrowserMsg::SelectRemoteFolder(
                            folder.provider_item_id.clone(),
                        )),
                    )
                }
                .view(),
            );
            if expanded {
                self.render_remote_places_tree(&folder.provider_item_id, depth + 1, rows);
            }
        }
    }
}
