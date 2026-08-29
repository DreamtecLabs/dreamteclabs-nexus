use std::rc::Rc;

use html::IntoPropValue;
use yew::virtual_dom::{Key, VComp, VNode};

use pwt::css::{self, Display, FlexFit};
use pwt::prelude::*;
use pwt::state::{NavigationContextExt, Selection};
use pwt::widget::nav::{Menu, MenuItem, NavigationDrawer};
use pwt::widget::{Container, Panel, Row, SelectionView, SelectionViewRenderInfo};

use proxmox_yew_comp::{AclContext, NotesView, XTermJs};

use pdm_api_types::remotes::RemoteType;
use pdm_api_types::{PRIV_SYS_AUDIT, PRIV_SYS_MODIFY};

use crate::ceph::CephView;
use crate::configuration::subscription_panel::SubscriptionPanel;
use crate::configuration::subscription_registry::SubscriptionRegistryProps;
use crate::configuration::views::ViewGrid;
use crate::dashboard::view::View;
use crate::remotes::RemotesPanel;
use crate::sdn::ZoneTree;
use crate::sdn::evpn::EvpnPanel;
use crate::{
    AccessControl, CertificatesPanel, RemoteListCacheEntry, ServerAdministration,
    SystemConfiguration,
};

#[path = "nexus/mod.rs"]
mod nexus;
use nexus::{NexusHome, NexusInventory};

use pwt_macros::builder;

#[derive(Clone, PartialEq, Properties)]
#[builder]
pub struct MainMenu {
    #[builder(IntoPropValue, into_prop_value)]
    #[prop_or_default]
    pub username: Option<AttrValue>,

    #[builder(IntoPropValue, into_prop_value)]
    #[prop_or_default]
    pub remote_list_loading: bool,

    #[builder]
    #[prop_or_default]
    pub remote_list: Vec<RemoteListCacheEntry>,

    #[builder]
    #[prop_or_default]
    pub view_list: Vec<String>,

    /// Notifies the parent app of the currently active top-level menu entry
    /// (e.g. "dashboard", "guests", "remote-homelab"), so it can be shown
    /// elsewhere in the shell, e.g. as a breadcrumb in the top bar.
    #[builder_cb(IntoEventCallback, into_event_callback, String)]
    #[prop_or_default]
    pub on_active_change: Option<Callback<String>>,
}

impl MainMenu {
    pub fn new() -> Self {
        yew::props!(Self {})
    }
}

pub enum Msg {
    Select(Key),
    UpdateAcl(AclContext),
}

pub struct PdmMainMenu {
    active: Key,
    menu_selection: Selection,
    acl_context: AclContext,
    _acl_context_listener: ContextHandle<AclContext>,
}

fn register_view(
    menu: &mut Menu,
    view: &mut SelectionView,
    text: impl Into<String>,
    id: &str,
    icon_class: Option<&'static str>,
    renderer: impl 'static + Fn(&SelectionViewRenderInfo) -> Html,
) {
    view.add_builder(id, renderer);
    menu.add_item(
        MenuItem::new(text.into())
            .key(id.to_string())
            .icon_class(icon_class),
    );
}

fn register_submenu(
    menu: &mut Menu,
    view: &mut SelectionView,
    text: impl Into<String>,
    id: &str,
    icon_class: Option<&'static str>,
    renderer: impl 'static + Fn(&SelectionViewRenderInfo) -> Html,
    submenu: Menu,
) {
    view.add_builder(id, renderer);
    menu.add_item(
        MenuItem::new(text.into())
            .key(id.to_string())
            .icon_class(icon_class)
            .submenu(submenu),
    );
}

impl Component for PdmMainMenu {
    type Message = Msg;
    type Properties = MainMenu;

    fn create(ctx: &Context<Self>) -> Self {
        let (acl_context, acl_context_listener) = ctx
            .link()
            .context(ctx.link().callback(Msg::UpdateAcl))
            .expect("acl context not present");

        Self {
            active: Key::from("dashboard"),
            menu_selection: Selection::new(),
            acl_context,
            _acl_context_listener: acl_context_listener,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Select(key) => {
                if let Some(on_active_change) = &ctx.props().on_active_change {
                    on_active_change.emit(key.to_string());
                }
                self.active = key;
                true
            }
            Msg::UpdateAcl(acl_context) => {
                self.acl_context = acl_context;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let scope = ctx.link().clone();
        let props = ctx.props();

        let route_view = match ctx.link().nav_context() {
            Some(nav) => match nav.path().split_once("-") {
                Some(("view", view)) => Some(view.to_string()),
                _ => None,
            },
            None => None,
        };

        let mut content = SelectionView::new()
            .class(FlexFit)
            .selection(self.menu_selection.clone());
        let mut menu = Menu::new();

        register_view(
            &mut menu,
            &mut content,
            "Dashboard",
            "dashboard",
            Some("fa fa-th-large"),
            move |_| html! { <NexusHome/> },
        );

        register_view(
            &mut menu,
            &mut content,
            "Inventory",
            "guests",
            Some("fa fa-cubes"),
            move |_| html! { <NexusInventory/> },
        );

        let mut infrastructure = Menu::new();
        register_view(
            &mut infrastructure,
            &mut content,
            "All remotes",
            "remotes",
            Some("fa fa-server"),
            |_| {
                Container::new()
                    .class("pwt-content-spacer")
                    .class(pwt::css::FlexFit)
                    .with_child(html! {<RemotesPanel/>})
                    .into()
            },
        );

        for remote in props
            .remote_list
            .iter()
            .filter(|remote| remote.ty == RemoteType::Pve)
        {
            register_view(
                &mut infrastructure,
                &mut content,
                &remote.id,
                &format!("remote-{}", remote.id),
                Some("fa fa-server"),
                {
                    let remote = remote.clone();
                    move |_| crate::pve::PveRemote::new(remote.id.clone()).into()
                },
            );
        }

        register_submenu(
            &mut menu,
            &mut content,
            "Infrastructure",
            "infrastructure",
            Some(if props.remote_list_loading {
                "fa fa-refresh fa-spin"
            } else {
                "fa fa-server"
            }),
            |_| {
                Container::new()
                    .class("pwt-content-spacer")
                    .class(pwt::css::FlexFit)
                    .with_child(html! {<RemotesPanel/>})
                    .into()
            },
            infrastructure,
        );

        let mut backups = Menu::new();
        let mut has_pbs = false;
        for remote in props
            .remote_list
            .iter()
            .filter(|remote| remote.ty == RemoteType::Pbs)
        {
            has_pbs = true;
            register_view(
                &mut backups,
                &mut content,
                &remote.id,
                &format!("remote-{}", remote.id),
                Some("fa fa-database"),
                {
                    let remote = remote.clone();
                    move |_| crate::pbs::PbsRemote::new(remote.id.clone()).into()
                },
            );
        }
        if has_pbs {
            register_submenu(
                &mut menu,
                &mut content,
                "Backups",
                "backups",
                Some("fa fa-database"),
                |_| {
                    Container::new()
                        .class("pwt-content-spacer")
                        .class(pwt::css::FlexFit)
                        .with_child(html! {<RemotesPanel/>})
                        .into()
                },
                backups,
            );
        }

        let mut networking = Menu::new();
        register_view(
            &mut networking,
            &mut content,
            "SDN",
            "sdn",
            Some("fa fa-sitemap"),
            |_| ZoneTree::new().into(),
        );
        register_view(
            &mut networking,
            &mut content,
            "EVPN",
            "evpn",
            Some("fa fa-share-alt"),
            |_| EvpnPanel::new().into(),
        );
        register_submenu(
            &mut menu,
            &mut content,
            "Networking",
            "networking",
            Some("fa fa-sitemap"),
            |_| ZoneTree::new().into(),
            networking,
        );

        register_view(
            &mut menu,
            &mut content,
            "Storage",
            "ceph",
            Some("fa fa-database"),
            |_| CephView::new().into(),
        );

        let mut views_menu = Menu::new();
        let mut found = false;
        for view_name in &props.view_list {
            let view_name = view_name.to_string();
            if route_view.as_ref() == Some(&view_name) {
                found = true;
            }
            register_view(
                &mut views_menu,
                &mut content,
                view_name.clone(),
                &format!("view-{view_name}"),
                Some("fa fa-plus-square-o"),
                move |_| View::new(Some(view_name.clone().into())).into(),
            );
        }
        if let (false, Some(view_name)) = (found, route_view) {
            register_view(
                &mut views_menu,
                &mut content,
                view_name.clone(),
                &format!("view-{view_name}"),
                Some("fa fa-plus-square-o"),
                move |_| View::new(Some(view_name.clone().into())).into(),
            );
        }
        register_submenu(
            &mut menu,
            &mut content,
            "Views",
            "views",
            Some("fa fa-columns"),
            |_| ViewGrid::new().into(),
            views_menu,
        );

        let mut settings = Menu::new();
        register_view(
            &mut settings,
            &mut content,
            "System",
            "configuration",
            Some("fa fa-sliders"),
            |_| html! { <SystemConfiguration/> },
        );
        register_view(
            &mut settings,
            &mut content,
            "Access Control",
            "access",
            Some("fa fa-key"),
            |_| html! {<AccessControl/>},
        );
        register_view(
            &mut settings,
            &mut content,
            "Certificates",
            "certificates",
            Some("fa fa-certificate"),
            |_| html! {<CertificatesPanel/>},
        );
        register_view(
            &mut settings,
            &mut content,
            "Subscription",
            "subscription",
            Some("fa fa-support"),
            |_| {
                Panel::new()
                    .class(css::FlexFit)
                    .title(tr!("Subscription"))
                    .with_child(SubscriptionPanel::new())
                    .into()
            },
        );
        register_view(
            &mut settings,
            &mut content,
            "Subscription Registry",
            "subscription-registry",
            Some("fa fa-id-card"),
            |_| SubscriptionRegistryProps::new().into(),
        );

        if self.acl_context.check_privs(&["system"], PRIV_SYS_AUDIT) {
            let allow_editing = self
                .acl_context
                .check_privs(&["system", "notes"], PRIV_SYS_MODIFY);
            register_view(
                &mut settings,
                &mut content,
                "Notes",
                "notes",
                Some("fa fa-sticky-note-o"),
                move |_| {
                    let mut notes = NotesView::new("/config/notes");
                    if allow_editing {
                        notes.set_on_submit(|notes| async move {
                            proxmox_yew_comp::http_put(
                                "/config/notes",
                                Some(serde_json::to_value(&notes)?),
                            )
                            .await
                        });
                    }
                    Container::new()
                        .class("pwt-content-spacer")
                        .class(pwt::css::FlexFit)
                        .with_child(notes)
                        .into()
                },
            );
        }

        let username = ctx.props().username.clone();
        register_view(
            &mut settings,
            &mut content,
            "Administration",
            "administration",
            Some("fa fa-wrench"),
            move |_| {
                ServerAdministration::new()
                    .username(username.clone())
                    .into()
            },
        );
        register_view(
            &mut settings,
            &mut content,
            "Shell",
            "shell",
            Some("fa fa-terminal"),
            |_| XTermJs::new().into(),
        );
        register_submenu(
            &mut menu,
            &mut content,
            "Settings",
            "settings",
            Some("fa fa-cog"),
            |_| html! { <SystemConfiguration/> },
            settings,
        );

        let drawer = NavigationDrawer::new(menu)
            .aria_label("Nexus navigation")
            .class("nexus-navigation")
            .class("pwt-border-end")
            .class(css::Flex::None)
            .width(238)
            .router(true)
            .default_active(self.active.to_string())
            .selection(self.menu_selection.clone())
            .on_select(Callback::from(move |id: Option<Key>| {
                let id = id.unwrap_or_else(|| Key::from(""));
                scope.send_message(Msg::Select(id))
            }));

        Container::new()
            .class(Display::Flex)
            .class(FlexFit)
            .with_child(
                Row::new()
                    .class(FlexFit)
                    .with_child(drawer)
                    .with_child(content),
            )
            .into()
    }
}

impl From<MainMenu> for VNode {
    fn from(val: MainMenu) -> Self {
        let comp = VComp::new::<PdmMainMenu>(Rc::new(val), None);
        VNode::from(comp)
    }
}
