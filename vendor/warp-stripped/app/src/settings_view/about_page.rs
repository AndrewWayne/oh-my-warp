use super::{
    settings_page::{
        MatchData, PageType, SettingsPageEvent, SettingsPageMeta, SettingsPageViewHandle,
        SettingsWidget,
    },
    SettingsSection,
};
use crate::{
    appearance::Appearance, channel::ChannelState, themes::theme::ColorScheme,
    workspace::WorkspaceAction,
};
use warpui::{
    assets::asset_cache::AssetSource,
    elements::{
        Align, CacheOption, ConstrainedBox, Container, CrossAxisAlignment, Element, Flex, Image,
        MainAxisAlignment, MouseStateHandle, ParentElement, Wrap,
    },
    ui_components::components::UiComponent,
    AppContext, Entity, View, ViewContext, ViewHandle,
};

#[cfg(feature = "omw_local")]
use warpui::{
    elements::{ClippedScrollStateHandle, ClippedScrollable, ScrollbarWidth, Text},
    fonts::Weight,
    ui_components::button::ButtonVariant,
    TypedActionView,
};

#[cfg(feature = "omw_local")]
const OMW_REPO_URL: &str = "https://github.com/AndrewWayne/oh-my-warp";
#[cfg(feature = "omw_local")]
const UPSTREAM_REPO_URL: &str = "https://github.com/warpdotdev/warp";
#[cfg(feature = "omw_local")]
const LICENSE_TEXT: &str = include_str!("../../../../../LICENSE");
#[cfg(feature = "omw_local")]
const LICENSE_BOX_HEIGHT: f32 = 280.;

#[cfg(feature = "omw_local")]
#[derive(Clone, Debug, PartialEq)]
pub enum AboutPageAction {
    OpenUrl(String),
}

pub struct AboutPageView {
    page: PageType<Self>,
}

impl AboutPageView {
    pub fn new(_ctx: &mut ViewContext<AboutPageView>) -> Self {
        AboutPageView {
            page: PageType::new_monolith(AboutPageWidget::default(), None, false),
        }
    }
}

impl Entity for AboutPageView {
    type Event = SettingsPageEvent;
}

impl View for AboutPageView {
    fn ui_name() -> &'static str {
        "AboutPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

#[cfg(feature = "omw_local")]
impl TypedActionView for AboutPageView {
    type Action = AboutPageAction;

    fn handle_action(&mut self, action: &AboutPageAction, ctx: &mut ViewContext<Self>) {
        match action {
            AboutPageAction::OpenUrl(url) => {
                ctx.open_url(url);
            }
        }
    }
}

#[derive(Default)]
struct AboutPageWidget {
    copy_version_button_mouse_state: MouseStateHandle,
    #[cfg(feature = "omw_local")]
    upstream_link_mouse_state: MouseStateHandle,
    #[cfg(feature = "omw_local")]
    omw_link_mouse_state: MouseStateHandle,
    #[cfg(feature = "omw_local")]
    license_scroll_state: ClippedScrollStateHandle,
}

impl SettingsWidget for AboutPageWidget {
    type View = AboutPageView;

    fn search_terms(&self) -> &str {
        #[cfg(feature = "omw_local")]
        {
            "about omw warp version license"
        }
        #[cfg(not(feature = "omw_local"))]
        {
            "about warp version"
        }
    }

    #[cfg(not(feature = "omw_local"))]
    fn render(
        &self,
        _view: &AboutPageView,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let ui_builder = appearance.ui_builder();

        let image_path = if theme.inferred_color_scheme() == ColorScheme::LightOnDark {
            "bundled/svg/warp-logo-with-light-title.svg"
        } else {
            "bundled/svg/warp-logo-with-dark-title.svg"
        };

        let version = ChannelState::app_version().unwrap_or("v#.##.###");

        let version_text = ui_builder
            .span(version.to_string())
            .with_soft_wrap()
            .build()
            .with_margin_top(16.)
            .finish();

        let copy_version_icon = appearance
            .ui_builder()
            .copy_button(16., self.copy_version_button_mouse_state.clone())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::CopyVersion(version));
            })
            .finish();

        let version_row = Wrap::row()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_children([
                version_text,
                Container::new(copy_version_icon)
                    .with_margin_top(16.)
                    .with_padding_left(6.)
                    .finish(),
            ]);

        Align::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    ConstrainedBox::new(
                        Image::new(
                            AssetSource::Bundled { path: image_path },
                            CacheOption::BySize,
                        )
                        .finish(),
                    )
                    .with_max_height(100.)
                    .with_max_width(350.)
                    .finish(),
                )
                .with_child(version_row.finish())
                .with_child(
                    ui_builder
                        .span("Copyright 2026 Warp")
                        .build()
                        .with_margin_top(16.)
                        .finish(),
                )
                .finish(),
        )
        .finish()
    }

    #[cfg(feature = "omw_local")]
    fn render(
        &self,
        _view: &AboutPageView,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let ui_builder = appearance.ui_builder();

        let image_path = if theme.inferred_color_scheme() == ColorScheme::LightOnDark {
            "bundled/svg/warp-logo-with-light-title.svg"
        } else {
            "bundled/svg/warp-logo-with-dark-title.svg"
        };

        let version = ChannelState::app_version().unwrap_or("v#.##.###");
        let active_color = theme.active_ui_text_color().into_solid();
        let muted_color = theme.nonactive_ui_text_color().into_solid();

        let logo = ConstrainedBox::new(
            Image::new(
                AssetSource::Bundled { path: image_path },
                CacheOption::BySize,
            )
            .finish(),
        )
        .with_max_height(100.)
        .with_max_width(350.)
        .finish();

        let version_text = ui_builder
            .span(version.to_string())
            .with_soft_wrap()
            .build()
            .with_margin_top(16.)
            .finish();

        let copy_version_icon = appearance
            .ui_builder()
            .copy_button(16., self.copy_version_button_mouse_state.clone())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::CopyVersion(version));
            })
            .finish();

        let version_row = Wrap::row()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_children([
                version_text,
                Container::new(copy_version_icon)
                    .with_margin_top(16.)
                    .with_padding_left(6.)
                    .finish(),
            ]);

        let app_name = Container::new(
            Text::new("omw — oh-my-warp".to_owned(), appearance.ui_font_family(), 18.)
                .with_color(active_color)
                .finish(),
        )
        .with_margin_top(20.)
        .finish();

        let description = Container::new(
            ConstrainedBox::new(
                Text::new(
                    "An audit-clean local build of the open source warp terminal. Cloud, AI, and signup features are stripped.".to_owned(),
                    appearance.ui_font_family(),
                    13.,
                )
                .with_color(muted_color)
                .soft_wrap(true)
                .finish(),
            )
            .with_max_width(420.)
            .finish(),
        )
        .with_margin_top(8.)
        .finish();

        let acknowledgements_header = Container::new(
            Text::new(
                "Acknowledgements".to_owned(),
                appearance.ui_font_family(),
                14.,
            )
            .with_color(active_color)
            .with_style(warpui::fonts::Properties::default().weight(Weight::Semibold))
            .finish(),
        )
        .with_margin_top(24.)
        .finish();

        let upstream_blurb = Container::new(
            Text::new(
                "Built on the open source warp terminal.".to_owned(),
                appearance.ui_font_family(),
                13.,
            )
            .with_color(muted_color)
            .finish(),
        )
        .with_margin_top(4.)
        .finish();

        let upstream_link = Container::new(
            appearance
                .ui_builder()
                .button(ButtonVariant::Link, self.upstream_link_mouse_state.clone())
                .with_text_label("warpdotdev/warp".to_owned())
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AboutPageAction::OpenUrl(
                        UPSTREAM_REPO_URL.to_owned(),
                    ));
                })
                .finish(),
        )
        .with_margin_top(2.)
        .finish();

        let project_home_header = Container::new(
            Text::new("Project home".to_owned(), appearance.ui_font_family(), 14.)
                .with_color(active_color)
                .with_style(warpui::fonts::Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_margin_top(20.)
        .finish();

        let omw_link = Container::new(
            appearance
                .ui_builder()
                .button(ButtonVariant::Link, self.omw_link_mouse_state.clone())
                .with_text_label("AndrewWayne/oh-my-warp".to_owned())
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AboutPageAction::OpenUrl(OMW_REPO_URL.to_owned()));
                })
                .finish(),
        )
        .with_margin_top(2.)
        .finish();

        let license_header = Container::new(
            Text::new("License".to_owned(), appearance.ui_font_family(), 14.)
                .with_color(active_color)
                .with_style(warpui::fonts::Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_margin_top(20.)
        .finish();

        let license_text = Text::new(
            LICENSE_TEXT.to_owned(),
            appearance.monospace_font_family(),
            11.,
        )
        .with_color(muted_color)
        .soft_wrap(true)
        .finish();

        let license_scroll = ConstrainedBox::new(
            ClippedScrollable::vertical(
                self.license_scroll_state.clone(),
                Container::new(license_text)
                    .with_uniform_padding(8.)
                    .finish(),
                ScrollbarWidth::Auto,
                warpui::elements::Fill::Solid(muted_color),
                warpui::elements::Fill::Solid(active_color),
                warpui::elements::Fill::None,
            )
            .finish(),
        )
        .with_max_height(LICENSE_BOX_HEIGHT)
        .with_max_width(520.)
        .finish();

        Align::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(logo)
                .with_child(version_row.finish())
                .with_child(app_name)
                .with_child(description)
                .with_child(acknowledgements_header)
                .with_child(upstream_blurb)
                .with_child(upstream_link)
                .with_child(project_home_header)
                .with_child(omw_link)
                .with_child(license_header)
                .with_child(license_scroll)
                .finish(),
        )
        .finish()
    }
}

impl SettingsPageMeta for AboutPageView {
    fn section() -> SettingsSection {
        SettingsSection::About
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<AboutPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<AboutPageView>) -> Self {
        SettingsPageViewHandle::About(view_handle)
    }
}
