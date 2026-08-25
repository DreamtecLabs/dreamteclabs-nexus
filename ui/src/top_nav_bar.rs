use std::rc::Rc;

use anyhow::Error;
use serde::Deserialize;

use pwt::AsyncAbortGuard;
use pwt::prelude::*;
use pwt::state::{Loader, ThemeObserver};
use pwt::widget::menu::{Menu, MenuButton, MenuEntry, MenuEvent, MenuItem};
use pwt::widget::{Button, Container, Row, ThemeModeSelector};
use yew::html::{IntoEventCallback, IntoPropValue};
use yew::virtual_dom::{VComp, VNode};

use proxmox_yew_comp::RunningTasksButton;
use proxmox_yew_comp::utils::set_location_href;
use proxmox_yew_comp::{LanguageDialog, TaskViewer, ThemeDialog, http_get};

use pwt_macros::builder;

use pbs_api_types::TaskListItem;
use pdm_api_types::RemoteUpid;

use crate::tasks::format_optional_remote_upid;
use crate::widget::SearchBox;

#[derive(Deserialize)]
pub struct VersionInfo {
    version: String,
    release: String,
}

async fn load_version() -> Result<VersionInfo, Error> {
    http_get("/version", None).await
}

#[derive(Clone, PartialEq, Properties)]
#[builder]
pub struct TopNavBar {
    running_tasks: Loader<Vec<TaskListItem>>,

    #[builder_cb(IntoEventCallback, into_event_callback, ())]
    #[prop_or_default]
    pub on_logout: Option<Callback<()>>,
    #[builder(IntoPropValue, into_prop_value)]
    #[prop_or_default]
    pub username: Option<String>,
}

impl TopNavBar {
    pub fn new(running_tasks: Loader<Vec<TaskListItem>>) -> Self {
        yew::props!(Self { running_tasks })
    }
}

#[derive(Clone)]
pub enum ViewState {
    LanguageDialog,
    ThemeDialog,
    OpenTask((String, Option<i64>)),
}

pub enum Msg {
    ThemeChanged,
    Load,
    LoadResult(Result<VersionInfo, Error>),
    ChangeView(Option<ViewState>),
}

pub struct PdmTopNavBar {
    _theme_observer: ThemeObserver,
    version_info: Option<VersionInfo>,
    view_state: Option<ViewState>,
    abort_guard: Option<AsyncAbortGuard>,
}

impl Component for PdmTopNavBar {
    type Message = Msg;
    type Properties = TopNavBar;

    fn create(ctx: &Context<Self>) -> Self {
        let props = ctx.props();
        let theme_observer = ThemeObserver::new(ctx.link().callback(|_| Msg::ThemeChanged));
        if props.username.is_some() {
            ctx.link().send_message(Msg::Load);
        }
        Self {
            _theme_observer: theme_observer,
            version_info: None,
            view_state: None,
            abort_guard: None,
        }
    }

    fn changed(&mut self, ctx: &Context<Self>, old_props: &Self::Properties) -> bool {
        if ctx.props().username != old_props.username {
            if ctx.props().username.is_some() {
                ctx.link().send_message(Msg::Load);
            } else {
                self.version_info = None;
            }
        }
        true
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::ChangeView(view_state) => {
                self.view_state = view_state;
                true
            }
            Msg::ThemeChanged => true,
            Msg::Load => {
                let link = ctx.link().clone();
                self.abort_guard.replace(AsyncAbortGuard::spawn(async move {
                    link.send_message(Msg::LoadResult(load_version().await))
                }));
                true
            }
            Msg::LoadResult(result) => {
                self.version_info = result.ok();
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let on_logout = props
            .on_logout
            .clone()
            .map(|cb| Callback::from(move |_event: MenuEvent| cb.emit(())));

        let menu = Menu::new()
            .with_item(
                MenuItem::new(tr!("Language"))
                    .icon_class("fa fa-language")
                    .on_select(
                        ctx.link()
                            .callback(|_| Msg::ChangeView(Some(ViewState::LanguageDialog))),
                    ),
            )
            .with_item(
                MenuItem::new(tr!("Theme"))
                    .icon_class("fa fa-desktop")
                    .on_select(
                        ctx.link()
                            .callback(|_| Msg::ChangeView(Some(ViewState::ThemeDialog))),
                    ),
            )
            .with_item(MenuEntry::Separator)
            .with_item(
                MenuItem::new(tr!("Logout"))
                    .icon_class("fa fa-sign-out")
                    .on_select(on_logout),
            );

        let mut actions = Row::new()
            .class("pwt-align-items-center")
            .gap(2)
            .with_child(ThemeModeSelector::new().class("pwt-scheme-neutral-alt"));

        if let Some(username) = &props.username {
            actions.add_child(
                RunningTasksButton::new(props.running_tasks.clone())
                    .on_show_task(
                        ctx.link()
                            .callback(|info| Msg::ChangeView(Some(ViewState::OpenTask(info)))),
                    )
                    .buttons(vec![
                        Button::new(tr!("Local Tasks"))
                            .on_activate(move |_| set_location_href("#/administration/tasks")),
                        Button::new(tr!("Remote Tasks"))
                            .on_activate(move |_| set_location_href("#/remotes/tasks")),
                    ])
                    .render(|item: &TaskListItem| {
                        format_optional_remote_upid(&item.upid, true).into()
                    }),
            );
            actions.add_child(
                MenuButton::new(username.clone())
                    .icon_class("fa fa-user-circle")
                    .show_arrow(true)
                    .menu(menu),
            );
        }

        let dialog: Option<Html> = self.view_state.as_ref().map(|state| match state {
            ViewState::LanguageDialog => LanguageDialog::new()
                .on_close(ctx.link().callback(|_| Msg::ChangeView(None)))
                .into(),
            ViewState::ThemeDialog => ThemeDialog::new()
                .on_close(ctx.link().callback(|_| Msg::ChangeView(None)))
                .into(),
            ViewState::OpenTask((task_id, _)) => {
                let base_url = task_id
                    .parse::<RemoteUpid>()
                    .ok()
                    .map(|upid| format!("/{}/remotes/{}/tasks", upid.remote_type(), upid.remote()));
                TaskViewer::new(task_id)
                    .base_url(base_url.unwrap_or("/nodes/localhost/tasks".to_string()))
                    .on_close(ctx.link().callback(|_| Msg::ChangeView(None)))
                    .into()
            }
        });

        let engine = self
            .version_info
            .as_ref()
            .map(|info| format!("PDM {}.{}", info.version, info.release))
            .unwrap_or_else(|| "PDM engine".to_string());

        Row::new()
            .attribute("role", "banner")
            .attribute("aria-label", "DreamtecLabs Nexus")
            .class("nexus-topbar")
            .class("pwt-justify-content-space-between pwt-align-items-center")
            .class("pwt-border-bottom")
            .padding(2)
            .with_child(html! {
                <>
                    <style>{r#"
                        .nexus-topbar{background:#fff!important;color:#0b1220!important;border-bottom:1px solid #dfe5ee!important;box-shadow:0 1px 4px rgba(15,23,42,.045);font-family:'Roboto Flex',Roboto,Arial,sans-serif!important}
                        .nexus-topbar button{color:#111827!important}
                        .nexus-navigation{background:#fff!important;color:#111827!important;border-right:1px solid #dfe5ee!important;font-family:'Roboto Flex',Roboto,Arial,sans-serif!important}
                        .nexus-navigation a,.nexus-navigation button{color:#111827!important;font-weight:520!important}
                        .nexus-navigation a:hover,.nexus-navigation button:hover{background:#f3f6fb!important;color:#0b1220!important}
                        .nexus-navigation [aria-current='page'],.nexus-navigation .pwt-nav-item-active{background:#e9f0ff!important;color:#1d4ed8!important;font-weight:700!important}
                    "#}</style>
                    <div style="display:flex;align-items:center;gap:12px;min-width:238px;padding-left:3px;">
                        <div style="position:relative;width:36px;height:36px;flex:0 0 36px;">
                            <span style="position:absolute;width:11px;height:34px;left:12px;top:1px;border-radius:8px;background:linear-gradient(180deg,#60a5fa,#2563eb);transform:rotate(43deg);box-shadow:0 2px 5px rgba(37,99,235,.20);"></span>
                            <span style="position:absolute;width:11px;height:34px;left:12px;top:1px;border-radius:8px;background:linear-gradient(180deg,#93c5fd,#4f46e5);transform:rotate(-43deg);box-shadow:0 2px 5px rgba(79,70,229,.16);"></span>
                        </div>
                        <div>
                            <div style="font-size:17px;font-weight:800;line-height:1;letter-spacing:-.025em;color:#070d18;">{"NEXUS"}</div>
                            <div style="font-size:10px;font-weight:600;color:#334155;margin-top:4px;">{"DreamtecLabs Nexus"}</div>
                            <div style="font-size:9px;color:#64748b;margin-top:2px;">{engine}</div>
                        </div>
                    </div>
                </>
            })
            .with_flex_spacer()
            .with_child(
                Container::new()
                    .width(460)
                    .with_child(SearchBox::new()),
            )
            .with_flex_spacer()
            .with_child(actions)
            .with_optional_child(dialog)
            .into()
    }
}

impl From<TopNavBar> for VNode {
    fn from(val: TopNavBar) -> Self {
        let comp = VComp::new::<PdmTopNavBar>(Rc::new(val), None);
        VNode::from(comp)
    }
}