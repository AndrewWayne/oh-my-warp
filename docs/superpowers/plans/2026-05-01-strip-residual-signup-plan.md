# Strip Residual Signup / Warp-Brand UI from `vendor/warp-stripped` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the remaining user-facing signup/Warp-brand surfaces and dead-but-compiled-in URLs in the `omw-warp-oss` preview build, so a `--features omw_local` build shows omw-flavored copy and zero `warpdotdev/warp` URLs in user surfaces.

**Architecture:** Source-level `#[cfg(feature = "omw_local")]` gates on user-visible strings, plus removing one entry (`open_warp_launch_modal`) from `omw_default` to suppress the OpenWarpLaunchModal trigger. No module-level rewrites. The default cloud build (no flag) is preserved unchanged.

**Tech Stack:** Rust 1.92.0 (pinned via `vendor/warp-stripped/rust-toolchain.toml`), Cargo workspace.

**Spec:** [`docs/superpowers/specs/2026-05-01-strip-residual-signup-design.md`](../specs/2026-05-01-strip-residual-signup-design.md)

---

## File Structure

This plan touches 12 files across `vendor/warp-stripped/` only. Each task identifies exact files, with line numbers from inspection on 2026-05-01.

**Inline banner**
- `vendor/warp-stripped/app/src/terminal/view/inline_banner/anonymous_user_ai_sign_up.rs` — banner constants and render

**Settings**
- `vendor/warp-stripped/app/src/settings_view/main_page.rs:317–403` — `render_anonymous_account_info`
- `vendor/warp-stripped/app/src/settings_view/about_page.rs` — full About page widget rebuild

**Menus**
- `vendor/warp-stripped/app/src/app_menus.rs:978–995` — `make_new_help_menu`
- `vendor/warp-stripped/app/src/workspace/mod.rs:1156` — keybinding label

**Pane / launch modal**
- `vendor/warp-stripped/app/src/pane_group/pane/get_started_view.rs:232,242` — title + tagline strings
- `vendor/warp-stripped/app/Cargo.toml:593` — drop one entry from `omw_default`

**Constants & dead strings**
- `vendor/warp-stripped/app/src/util/links.rs:5` — `GITHUB_ISSUES_URL`
- `vendor/warp-stripped/app/src/auth/auth_view_body.rs:619,622,625,651,654,1009`
- `vendor/warp-stripped/app/src/auth/needs_sso_link_view.rs:79`
- `vendor/warp-stripped/app/src/auth/auth_override_warning_body.rs:31`
- `vendor/warp-stripped/app/src/workspace/view/build_plan_migration_modal.rs:518`

**Verification + docs**
- `vendor/warp-stripped/scripts/audit-no-cloud.sh` — re-run, no edits expected
- `TODO.md` — mark v0.0.2 cleanup complete

---

## Conventions

Throughout this plan:

- **omw_local build:** `cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local`
- **Default build (reverse check, proves fork-strategy preservation):** `cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss`
- **Full preview build:** `bash scripts/build-mac-dmg.sh 0.0.2-rc.1` (only run once at the end of the plan).
- **All edits are inside `vendor/warp-stripped/`.** Paths in this plan are relative to the umbrella repo root unless noted.
- **License-header rule (CLAUDE.md §5):** every touched file already has the AGPL header; preserve it unchanged.
- **Brand-rule (CLAUDE.md §5):** never write capitalized `Warp` in any new code/copy. Lowercase `warp` is allowed in product copy. Capitalized `Warp` may appear only in `LICENSE` text or upstream-attribution comments.
- **Commits:** one task = one commit. Subject ≤ 72 chars, imperative voice. Use the same `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` trailer as the existing repo style.

---

## Task 1: Verify baseline builds clean

**Files:** none (verification-only).

- [ ] **Step 1: Verify omw_local build is currently green**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors. If errors appear, **stop and report** — the plan assumes a green baseline.

- [ ] **Step 2: Verify default build is currently green**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors.

- [ ] **Step 3: Snapshot the audit script result**

```bash
ls vendor/warp-stripped/scripts/audit-no-cloud.sh
```

Expected: file exists. (We'll re-run it at the end; this just confirms the gate is present.)

No commit in this task.

---

## Task 2: A1 — Inline banner copy + drop Sign Up button

**Files:**
- Modify: `vendor/warp-stripped/app/src/terminal/view/inline_banner/anonymous_user_ai_sign_up.rs:20–23, 211–216`

- [ ] **Step 1: Cfg-gate the TITLE and CONTENT constants**

Open `vendor/warp-stripped/app/src/terminal/view/inline_banner/anonymous_user_ai_sign_up.rs`. Find lines 20–23:

```rust
const TITLE: &str = "Login for AI";
const CONTENT: &str =
    "AI features are unavailable for logged-out users. Create an account to use AI.";
const SIGN_UP_BUTTON_TEXT: &str = "Sign Up";
```

Replace with:

```rust
#[cfg(not(feature = "omw_local"))]
const TITLE: &str = "Login for AI";
#[cfg(not(feature = "omw_local"))]
const CONTENT: &str =
    "AI features are unavailable for logged-out users. Create an account to use AI.";
#[cfg(not(feature = "omw_local"))]
const SIGN_UP_BUTTON_TEXT: &str = "Sign Up";

#[cfg(feature = "omw_local")]
const TITLE: &str = "Welcome to omw (oh-my-warp)";
#[cfg(feature = "omw_local")]
const CONTENT: &str =
    "Project built on the open source warp terminal. AI is disabled in this build.";
#[cfg(feature = "omw_local")]
const SIGN_UP_BUTTON_TEXT: &str = "";
```

`SIGN_UP_BUTTON_TEXT` is kept (empty under omw_local) so the existing function signature `render_three_column_inline_banner(..., button_text: &str, ...)` doesn't need to change. The button itself is gated out in the next step.

- [ ] **Step 2: Cfg-gate the Sign Up button rendering**

In the same file, find the block that renders the Sign Up button (lines ~178–215, starting with `// Sign Up Button` and ending with `buttons_column.add_child(`). Wrap the whole block in `#[cfg(not(feature = "omw_local"))]`:

```rust
    #[cfg(not(feature = "omw_local"))]
    {
        // Sign Up Button
        let button_styles = UiComponentStyles {
            font_color: Some(active_text_color),
            font_size: Some(button_text_size),
            font_weight: Some(warpui::fonts::Weight::Semibold),
            border_color: Some(warpui::elements::Fill::Solid(content_text_color)),
            border_width: Some(1.0),
            border_radius: Some(CornerRadius::with_all(Radius::Pixels(
                INLINE_BANNER_BUTTON_PADDING,
            ))),
            ..Default::default()
        };

        let button_on_click_event =
            TerminalAction::AnonymousUserAISignUpBanner(AnonymousUserLoginBannerAction::SignUp);
        let button = appearance
            .ui_builder()
            .button_with_custom_styles(
                ButtonVariant::Text,
                button_mouse_state,
                default_button_styles,
                Some(hovered_and_clicked_styles),
                Some(hovered_and_clicked_styles),
                Some(hovered_and_clicked_styles),
            )
            .with_text_label(button_text.to_string())
            .with_style(button_styles)
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(button_on_click_event.clone());
            })
            .finish();

        buttons_column.add_child(
            Container::new(button)
                .with_margin_left(INLINE_BANNER_MARGIN_BETWEEN_BUTTONS)
                .finish(),
        );
    }
```

The `button_mouse_state` parameter and `_button_text: &str` parameter are unused under omw_local. Add `#[cfg_attr(feature = "omw_local", allow(unused))]` directly above the function signature `fn render_three_column_inline_banner(`:

```rust
#[cfg_attr(feature = "omw_local", allow(unused))]
fn render_three_column_inline_banner(
    appearance: &Appearance,
    title: &str,
    content: &str,
    button_text: &str,
    button_mouse_state: MouseStateHandle,
    close_button_mouse_state: MouseStateHandle,
) -> Box<dyn Element> {
```

- [ ] **Step 3: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no warnings about the gated code. If `unused_variables` warnings appear for `button_mouse_state` or `button_text`, the `#[cfg_attr]` is on the wrong line — re-check.

- [ ] **Step 4: Build under default (reverse check)**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors. Confirms the default cloud build still includes the original banner.

- [ ] **Step 5: Grep verification**

```bash
grep -c "Welcome to omw" vendor/warp-stripped/app/src/terminal/view/inline_banner/anonymous_user_ai_sign_up.rs
grep -c "Login for AI" vendor/warp-stripped/app/src/terminal/view/inline_banner/anonymous_user_ai_sign_up.rs
```

Expected: both return `1` (each string appears once, behind its own cfg arm).

- [ ] **Step 6: Commit**

```bash
git add vendor/warp-stripped/app/src/terminal/view/inline_banner/anonymous_user_ai_sign_up.rs
git commit -m "$(cat <<'EOF'
Gate inline AI signup banner copy + button under omw_local

Replaces the "Login for AI / Sign Up" inline banner with an omw welcome
message and removes the Sign Up button under omw_local. Default build
unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: A2 — Settings Account page (replace anonymous Sign up)

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/main_page.rs:317–403`

- [ ] **Step 1: Add cfg-gated minimal renderer above the existing one**

Open `vendor/warp-stripped/app/src/settings_view/main_page.rs`. Line 318 starts `fn render_anonymous_account_info`. Replace the entire function (lines 318–403) with:

```rust
    #[cfg(feature = "omw_local")]
    fn render_anonymous_account_info(
        &self,
        _auth_state: &AuthState,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let text_color = theme.nonactive_ui_text_color().into_solid();
        Container::new(
            Text::new(
                "Standalone build — sign-in is disabled. See the About page for project info.".to_owned(),
                appearance.ui_font_family(),
                14.,
            )
            .with_color(text_color)
            .soft_wrap(true)
            .finish(),
        )
        .with_padding_top(16.)
        .with_padding_bottom(16.)
        .finish()
    }

    #[cfg(not(feature = "omw_local"))]
    fn render_anonymous_account_info(
        &self,
        auth_state: &AuthState,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let button_styles = UiComponentStyles {
            font_size: Some(14.),
            font_weight: Some(Weight::Semibold),
            border_radius: Some(CornerRadius::with_all(Radius::Pixels(4.))),
            padding: Some(Coords {
                top: 12.,
                bottom: 12.,
                left: 40.,
                right: 40.,
            }),
            ..Default::default()
        };

        let user_info = appearance
            .ui_builder()
            .button(
                ButtonVariant::Accent,
                self.ui_state_handles.anonymous_user_sign_up_button.clone(),
            )
            .with_style(button_styles)
            .with_text_label("Sign up".to_owned())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(MainPageAction::SignupAnonymousUser);
            })
            .finish();

        let mut plan_info = Flex::column()
            .with_main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .with_cross_axis_alignment(CrossAxisAlignment::End);
        let current_user_id = auth_state.user_id().unwrap_or_default();

        plan_info.add_child(render_customer_type_badge(appearance, "Free".into()));
        plan_info.add_child(
            Container::new(
                appearance
                    .ui_builder()
                    .button(
                        ButtonVariant::Link,
                        self.ui_state_handles.upgrade_link.clone(),
                    )
                    .with_text_and_icon_label(
                        TextAndIcon::new(
                            TextAndIconAlignment::IconFirst,
                            "Compare plans",
                            Icon::CoinsStacked.to_warpui_icon(appearance.theme().accent()),
                            MainAxisSize::Min,
                            MainAxisAlignment::Center,
                            vec2f(14., 14.),
                        )
                        .with_inner_padding(4.),
                    )
                    .build()
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(MainPageAction::Upgrade {
                            team_uid: None,
                            user_id: current_user_id,
                        });
                    })
                    .finish(),
            )
            .with_margin_top(8.)
            .finish(),
        );

        Flex::row()
            .with_child(
                Shrinkable::new(
                    1.0,
                    Flex::row()
                        .with_child(user_info)
                        .with_main_axis_alignment(MainAxisAlignment::Start)
                        .with_main_axis_size(MainAxisSize::Max)
                        .finish(),
                )
                .finish(),
            )
            .with_child(Align::new(plan_info.finish()).right().finish())
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .finish()
    }
```

The omw_local arm only uses `Text`, `Container`, and the existing imports. The default arm is unchanged.

- [ ] **Step 2: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors.

If you see warnings about `ui_state_handles.anonymous_user_sign_up_button` or `ui_state_handles.upgrade_link` being unused in omw_local: that's expected (they're only used in the default arm). Add `#[cfg_attr(feature = "omw_local", allow(dead_code))]` to the `AccountWidgetStateHandles` struct fields (lines ~305–310) if rustc complains. Most likely it doesn't, because struct fields without `#[allow(dead_code)]` already suppress this warning when the field is held in a `Default::default()` initializer.

- [ ] **Step 3: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Grep verification**

```bash
grep -c "Standalone build" vendor/warp-stripped/app/src/settings_view/main_page.rs
grep -c "Sign up" vendor/warp-stripped/app/src/settings_view/main_page.rs
```

Expected: `Standalone build` returns `1` (in the omw_local arm). `Sign up` returns `1` (in the default arm only).

- [ ] **Step 5: Commit**

```bash
git add vendor/warp-stripped/app/src/settings_view/main_page.rs
git commit -m "$(cat <<'EOF'
Gate Account page anonymous Sign up button under omw_local

Under omw_local the Account page renders a single muted notice instead
of the Sign up / Compare plans / Upgrade UI. Default build unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: A3 — Rebuild Settings About page

This is the largest task. We rebuild `AboutPageWidget::render` under `omw_local` to show app description, two GitHub links, and a scrollable embedded LICENSE.

**Files:**
- Modify: `vendor/warp-stripped/app/src/settings_view/about_page.rs` (full rewrite under cfg)

- [ ] **Step 1: Replace the file content**

Open `vendor/warp-stripped/app/src/settings_view/about_page.rs` and replace its entire content with:

```rust
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
};

#[cfg(feature = "omw_local")]
const OMW_REPO_URL: &str = "https://github.com/AndrewWayne/oh-my-warp";
#[cfg(feature = "omw_local")]
const UPSTREAM_REPO_URL: &str = "https://github.com/warpdotdev/warp";
#[cfg(feature = "omw_local")]
const LICENSE_TEXT: &str = include_str!("../../../../../LICENSE");
#[cfg(feature = "omw_local")]
const LICENSE_BOX_HEIGHT: f32 = 280.;

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
        "about omw warp version license"
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
            Text::new(
                "An audit-clean local build of the open source warp terminal. Cloud, AI, and signup features are stripped.".to_owned(),
                appearance.ui_font_family(),
                13.,
            )
            .with_color(muted_color)
            .soft_wrap(true)
            .finish(),
        )
        .with_margin_top(8.)
        .with_max_width(420.)
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

        let upstream_link = appearance
            .ui_builder()
            .button(ButtonVariant::Link, self.upstream_link_mouse_state.clone())
            .with_text_label("warpdotdev/warp".to_owned())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.open_url(UPSTREAM_REPO_URL);
            })
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

        let omw_link = appearance
            .ui_builder()
            .button(ButtonVariant::Link, self.omw_link_mouse_state.clone())
            .with_text_label("AndrewWayne/oh-my-warp".to_owned())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.open_url(OMW_REPO_URL);
            })
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
```

- [ ] **Step 2: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors.

If `include_str!` fails with "couldn't find file": the path `../../../../../LICENSE` is wrong. From `vendor/warp-stripped/app/src/settings_view/about_page.rs`, count back to repo root: `settings_view/` → `src/` → `app/` → `warp-stripped/` → `vendor/` → repo root = 5 levels. The file is `LICENSE`. So `../../../../../LICENSE` is correct. If still failing, run `realpath --relative-to=vendor/warp-stripped/app/src/settings_view/ LICENSE` from the repo root and use that result.

If `Text::with_style` doesn't exist or doesn't accept `Properties`: try `.with_style(warpui::fonts::Properties::default().weight(Weight::Semibold))` → if it errors, swap to using `Text::new(...)` followed by a separate weight setter, or fall back to the existing `ui_builder.span(...).with_weight(Weight::Semibold).build()` pattern visible in `main_page.rs`.

If `ButtonVariant::Link` `.on_click` closure can't access `ctx.open_url`: check the trait the closure parameter implements. If `ctx` is `&mut MouseEventContext` and lacks `open_url`, switch to dispatching `WorkspaceAction::OpenUrl` (if it exists) — there's a precedent in `app_menus.rs`'s `link_menu_item` helper. Confirm by `grep -n "fn open_url" vendor/warp-stripped/crates/warpui/src/`.

If `ClippedScrollable::vertical` arg count is wrong: re-read `vendor/warp-stripped/app/src/settings_view/settings_file_footer.rs:212–223` for the exact call site signature and match it.

- [ ] **Step 3: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors. The default arm of `render` is unchanged from the original file, so this should pass without surprises.

- [ ] **Step 4: Grep verification**

```bash
grep -c "An audit-clean local build" vendor/warp-stripped/app/src/settings_view/about_page.rs
grep -c "Copyright 2026 Warp" vendor/warp-stripped/app/src/settings_view/about_page.rs
grep -c "include_str" vendor/warp-stripped/app/src/settings_view/about_page.rs
```

Expected: `An audit-clean local build` returns `1`. `Copyright 2026 Warp` returns `1` (only in the default arm). `include_str` returns `1`.

- [ ] **Step 5: Commit**

```bash
git add vendor/warp-stripped/app/src/settings_view/about_page.rs
git commit -m "$(cat <<'EOF'
Rebuild About page under omw_local with project info + LICENSE

Under omw_local the About page now shows app name, description, link to
upstream warpdotdev/warp, link to AndrewWayne/oh-my-warp, and the full
repo LICENSE text in a scrollable container. Default cloud build keeps
the original logo + version + copyright layout.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: A4 — Help menu cleanup

**Files:**
- Modify: `vendor/warp-stripped/app/src/app_menus.rs:978–995`

- [ ] **Step 1: Restructure `make_new_help_menu`**

Find lines 978–995 in `vendor/warp-stripped/app/src/app_menus.rs`:

```rust
fn make_new_help_menu() -> Menu {
    #[cfg_attr(feature = "omw_local", allow(unused_mut))]
    let mut items = vec![
        link_menu_item("Warp Documentation...", links::USER_DOCS_URL.into()),
        link_menu_item("GitHub Issues...", links::GITHUB_ISSUES_URL.into()),
    ];

    #[cfg(not(feature = "omw_local"))]
    {
        items.insert(0, feedback_menu_item());
        items.push(link_menu_item(
            "Warp Slack Community...",
            links::SLACK_URL.into(),
        ));
    }

    Menu::new("Help", items)
}
```

Replace with:

```rust
fn make_new_help_menu() -> Menu {
    #[cfg(feature = "omw_local")]
    let items = vec![
        link_menu_item(
            "Project on GitHub...",
            "https://github.com/AndrewWayne/oh-my-warp".into(),
        ),
        link_menu_item(
            "Report an Issue...",
            "https://github.com/AndrewWayne/oh-my-warp/issues".into(),
        ),
    ];

    #[cfg(not(feature = "omw_local"))]
    let items = {
        let mut items = vec![
            feedback_menu_item(),
            link_menu_item("Warp Documentation...", links::USER_DOCS_URL.into()),
            link_menu_item("GitHub Issues...", links::GITHUB_ISSUES_URL.into()),
            link_menu_item("Warp Slack Community...", links::SLACK_URL.into()),
        ];
        items
    };

    Menu::new("Help", items)
}
```

(The default arm collapses the original two-step build into a single vec for clarity; behavior is identical — `feedback_menu_item` is still first, then the two `links::*` items, then Slack, in the same order as before.)

- [ ] **Step 2: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors.

If a warning appears about `unused: links::USER_DOCS_URL` or similar: that's because the omw_local arm doesn't reference `links::*`. Check whether `use crate::util::links;` (somewhere near the top of `app_menus.rs`) needs `#[cfg_attr(feature = "omw_local", allow(unused_imports))]`. Add it if so.

- [ ] **Step 3: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Grep verification**

```bash
grep -c "AndrewWayne/oh-my-warp" vendor/warp-stripped/app/src/app_menus.rs
grep -c "Warp Documentation" vendor/warp-stripped/app/src/app_menus.rs
```

Expected: first returns `2` (one for repo link, one for issues link). Second returns `1` (only in default arm).

- [ ] **Step 5: Commit**

```bash
git add vendor/warp-stripped/app/src/app_menus.rs
git commit -m "$(cat <<'EOF'
Replace Help menu items under omw_local with omw repo links

Under omw_local the Help menu shows "Project on GitHub..." and
"Report an Issue..." pointing at AndrewWayne/oh-my-warp. Default cloud
build keeps the original Feedback / Warp Documentation / GitHub Issues
/ Slack items.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: A5 — GetStartedView title and tagline

**Files:**
- Modify: `vendor/warp-stripped/app/src/pane_group/pane/get_started_view.rs:230–252`

- [ ] **Step 1: Cfg-gate the title and tagline literals**

Open `vendor/warp-stripped/app/src/pane_group/pane/get_started_view.rs`. Find lines around 230–252 — the `paragraph("Welcome to Warp")` and `paragraph("The Agentic Development Environment")` calls.

Just above the `Flex::column()` block at line 217, add two cfg-gated constants:

```rust
        #[cfg(not(feature = "omw_local"))]
        const GET_STARTED_TITLE: &str = "Welcome to Warp";
        #[cfg(feature = "omw_local")]
        const GET_STARTED_TITLE: &str = "Welcome to omw";

        #[cfg(not(feature = "omw_local"))]
        const GET_STARTED_TAGLINE: &str = "The Agentic Development Environment";
        #[cfg(feature = "omw_local")]
        const GET_STARTED_TAGLINE: &str = "Open-source terminal — local build";
```

Then replace the two `.paragraph("Welcome to Warp")` and `.paragraph("The Agentic Development Environment")` calls (lines 232 and 242 in the original file) with `.paragraph(GET_STARTED_TITLE)` and `.paragraph(GET_STARTED_TAGLINE)` respectively.

- [ ] **Step 2: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors.

- [ ] **Step 3: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Grep verification**

```bash
grep -c "Welcome to omw" vendor/warp-stripped/app/src/pane_group/pane/get_started_view.rs
grep -c "Welcome to Warp" vendor/warp-stripped/app/src/pane_group/pane/get_started_view.rs
```

Expected: each returns `1`.

- [ ] **Step 5: Commit**

```bash
git add vendor/warp-stripped/app/src/pane_group/pane/get_started_view.rs
git commit -m "$(cat <<'EOF'
Gate Get Started tab title + tagline under omw_local

The "New Tab → Get Started" surface now reads "Welcome to omw" with an
omw-flavored tagline under omw_local. Default build unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: B1 — Drop OpenWarpLaunchModal from omw_default features

**Files:**
- Modify: `vendor/warp-stripped/app/Cargo.toml:593`

- [ ] **Step 1: Remove the feature entry from omw_default**

Open `vendor/warp-stripped/app/Cargo.toml`. Around line 593 the `omw_default` feature list contains:

```toml
    "oz_launch_modal",
    "open_warp_launch_modal",
    "new_tab_styling",
```

Remove the `"open_warp_launch_modal",` line:

```toml
    "oz_launch_modal",
    "new_tab_styling",
```

The cargo feature `open_warp_launch_modal` itself stays defined at line 907 (`open_warp_launch_modal = []`) — only its inclusion in `omw_default` is removed.

- [ ] **Step 2: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors.

If the build fails referencing missing `FeatureFlag::OpenWarpLaunchModal` registration: that means somewhere in the codebase a non-`#[cfg]`-gated branch references the flag in a way that requires the registration. Re-add the feature and report — the feature flag has more dependencies than expected and we'll need a different approach (gating the modal trigger sites instead).

- [ ] **Step 3: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished`. The default `cloud` feature still pulls `open_warp_launch_modal` into the build (verified by `grep '"open_warp_launch_modal"' vendor/warp-stripped/app/Cargo.toml`), so the modal still registers in the default build.

- [ ] **Step 4: Grep verification**

```bash
grep -n "open_warp_launch_modal" vendor/warp-stripped/app/Cargo.toml
```

Expected: lines 907 (`open_warp_launch_modal = []`) and the cloud feature list (around line 459 / `default = ["omw_default", "cloud"]` block) — but **not** in the `omw_default = [...]` list anymore.

- [ ] **Step 5: Commit**

```bash
git add vendor/warp-stripped/app/Cargo.toml
git commit -m "$(cat <<'EOF'
Remove open_warp_launch_modal from omw_default

The OpenWarpLaunchModal ("Warp is now open-source" + warpdotdev/warp
link) is gated on FeatureFlag::OpenWarpLaunchModal, which is registered
only when the cargo feature is enabled. Removing it from omw_default
suppresses the modal trigger entirely in omw_local builds while leaving
upstream behavior unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: B2 — Cfg-gate `GITHUB_ISSUES_URL`

**Files:**
- Modify: `vendor/warp-stripped/app/src/util/links.rs:5`

- [ ] **Step 1: Add cfg-gated arms for `GITHUB_ISSUES_URL`**

Open `vendor/warp-stripped/app/src/util/links.rs`. Replace line 5:

```rust
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const GITHUB_ISSUES_URL: &str = "https://github.com/warpdotdev/Warp/issues";
```

with:

```rust
#[cfg(feature = "omw_local")]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const GITHUB_ISSUES_URL: &str = "https://github.com/AndrewWayne/oh-my-warp/issues";

#[cfg(not(feature = "omw_local"))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const GITHUB_ISSUES_URL: &str = "https://github.com/warpdotdev/Warp/issues";
```

- [ ] **Step 2: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors.

- [ ] **Step 3: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Grep verification**

```bash
grep -c "AndrewWayne/oh-my-warp/issues" vendor/warp-stripped/app/src/util/links.rs
grep -c "warpdotdev/Warp/issues" vendor/warp-stripped/app/src/util/links.rs
```

Expected: first returns `1`, second returns `2` (one in `GITHUB_ISSUES_URL` default arm + one in `feedback_form_url()` body).

- [ ] **Step 5: Commit**

```bash
git add vendor/warp-stripped/app/src/util/links.rs
git commit -m "$(cat <<'EOF'
Point GITHUB_ISSUES_URL at oh-my-warp under omw_local

Default build keeps upstream issues URL.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: B3 — Cfg-gate "Toggle Warp AI" keybinding label

**Files:**
- Modify: `vendor/warp-stripped/app/src/workspace/mod.rs:1156`

- [ ] **Step 1: Cfg-gate the label string**

Open `vendor/warp-stripped/app/src/workspace/mod.rs`. Find lines ~1149–1170 — the `EditableBinding::new("workspace:toggle_ai_assistant", ...)` block. The string `"Toggle Warp AI"` is the second argument.

Replace:

```rust
        EditableBinding::new(
            "workspace:toggle_ai_assistant",
            "Toggle Warp AI",
            WorkspaceAction::ToggleAIAssistant,
        )
```

with:

```rust
        EditableBinding::new(
            "workspace:toggle_ai_assistant",
            #[cfg(feature = "omw_local")]
            "Toggle AI Assistant",
            #[cfg(not(feature = "omw_local"))]
            "Toggle Warp AI",
            WorkspaceAction::ToggleAIAssistant,
        )
```

- [ ] **Step 2: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors. If `EditableBinding::new` doesn't accept attribute-on-argument syntax (rare in stable Rust): swap to a `#[cfg]`-defined `const` above the call and pass that const as the argument.

- [ ] **Step 3: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Commit**

```bash
git add vendor/warp-stripped/app/src/workspace/mod.rs
git commit -m "$(cat <<'EOF'
Rename "Toggle Warp AI" keybinding label to "Toggle AI Assistant" under omw_local

Default build unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: B4 — Blank dead "Warp" strings in unreachable surfaces

These edits target user-visible "Warp" strings in code paths that are *not* reachable in omw_local (auth modals, SSO link, build-plan migration) but whose strings would still ship in the binary. Each edit gates the literal under `#[cfg(feature = "omw_local")]` to an empty string.

**Files:**
- Modify: `vendor/warp-stripped/app/src/auth/auth_view_body.rs` lines 619, 622, 625, 651, 654, 1009
- Modify: `vendor/warp-stripped/app/src/auth/needs_sso_link_view.rs:79`
- Modify: `vendor/warp-stripped/app/src/auth/auth_override_warning_body.rs:31`
- Modify: `vendor/warp-stripped/app/src/workspace/view/build_plan_migration_modal.rs:518`

- [ ] **Step 1: Edit `auth_view_body.rs`**

Open `vendor/warp-stripped/app/src/auth/auth_view_body.rs`. The relevant strings live inside two `match` blocks (lines 617–628 and 650–656) and one literal call (~line 1009).

Replace lines 617–628 (the match in `render_force_login_disclaimer`):

```rust
        let text = match self.variant {
            AuthViewVariant::RequireLoginCloseable  => {
                "In order to use Warp's AI features or collaborate with others, please create an account."
            }
            AuthViewVariant::HitDriveObjectLimitCloseable => {
                "In order to create more objects in Warp Drive, please create an account."
            }
            AuthViewVariant::ShareRequirementCloseable => {
                "In order to share, please create an account."
            }
            _ => "",
        };
```

with:

```rust
        let text = match self.variant {
            #[cfg(not(feature = "omw_local"))]
            AuthViewVariant::RequireLoginCloseable => {
                "In order to use Warp's AI features or collaborate with others, please create an account."
            }
            #[cfg(not(feature = "omw_local"))]
            AuthViewVariant::HitDriveObjectLimitCloseable => {
                "In order to create more objects in Warp Drive, please create an account."
            }
            #[cfg(not(feature = "omw_local"))]
            AuthViewVariant::ShareRequirementCloseable => {
                "In order to share, please create an account."
            }
            _ => "",
        };
```

If `#[cfg]` on match arms breaks the exhaustiveness check (the `_` arm covers all in omw_local), the simpler alternative is to gate the inside of each arm:

```rust
        let text = match self.variant {
            AuthViewVariant::RequireLoginCloseable => {
                #[cfg(feature = "omw_local")]
                { "" }
                #[cfg(not(feature = "omw_local"))]
                { "In order to use Warp's AI features or collaborate with others, please create an account." }
            }
            AuthViewVariant::HitDriveObjectLimitCloseable => {
                #[cfg(feature = "omw_local")]
                { "" }
                #[cfg(not(feature = "omw_local"))]
                { "In order to create more objects in Warp Drive, please create an account." }
            }
            AuthViewVariant::ShareRequirementCloseable => {
                #[cfg(feature = "omw_local")]
                { "" }
                #[cfg(not(feature = "omw_local"))]
                { "In order to share, please create an account." }
            }
            _ => "",
        };
```

Use whichever compiles; prefer the first (cleaner). If the first errors with an exhaustiveness diagnostic, fall back to the second.

Replace lines 650–656 (the match in `render_header`):

```rust
        let text = match self.variant {
            AuthViewVariant::Initial => "Welcome to Warp!",
            AuthViewVariant::RequireLoginCloseable
            | AuthViewVariant::HitDriveObjectLimitCloseable
            | AuthViewVariant::ShareRequirementCloseable => "Sign up for Warp",
        };
```

with:

```rust
        #[cfg(feature = "omw_local")]
        let text = "";
        #[cfg(not(feature = "omw_local"))]
        let text = match self.variant {
            AuthViewVariant::Initial => "Welcome to Warp!",
            AuthViewVariant::RequireLoginCloseable
            | AuthViewVariant::HitDriveObjectLimitCloseable
            | AuthViewVariant::ShareRequirementCloseable => "Sign up for Warp",
        };
```

For line ~1009 (`"Welcome to Warp!"` in a different render context):

Find:

```rust
            "Welcome to Warp!",
```

Replace with:

```rust
            #[cfg(feature = "omw_local")]
            "",
            #[cfg(not(feature = "omw_local"))]
            "Welcome to Warp!",
```

If that breaks because the surrounding macro / function call rejects attribute-on-argument syntax: extract a local `let` above the call site, gate the let binding with `#[cfg]`, and use the variable.

- [ ] **Step 2: Edit `needs_sso_link_view.rs`**

Open `vendor/warp-stripped/app/src/auth/needs_sso_link_view.rs`. Find line 79:

```rust
            .with_detail("Click the button below to link your Warp account to your SSO provider.")
```

Replace with:

```rust
            .with_detail({
                #[cfg(feature = "omw_local")]
                { "" }
                #[cfg(not(feature = "omw_local"))]
                { "Click the button below to link your Warp account to your SSO provider." }
            })
```

- [ ] **Step 3: Edit `auth_override_warning_body.rs`**

Open `vendor/warp-stripped/app/src/auth/auth_override_warning_body.rs`. Find line 31:

```rust
const AUTH_OVERRIDE_DESCRIPTION: &str = "It looks like you logged into a Warp account through a web browser. If you continue, any personal Warp drive objects and preferences from this anonymous session with be permanently deleted.";
```

Replace with:

```rust
#[cfg(feature = "omw_local")]
const AUTH_OVERRIDE_DESCRIPTION: &str = "";
#[cfg(not(feature = "omw_local"))]
const AUTH_OVERRIDE_DESCRIPTION: &str = "It looks like you logged into a Warp account through a web browser. If you continue, any personal Warp drive objects and preferences from this anonymous session with be permanently deleted.";
```

- [ ] **Step 4: Edit `build_plan_migration_modal.rs`**

Open `vendor/warp-stripped/app/src/workspace/view/build_plan_migration_modal.rs`. Find line 518:

```rust
            "Welcome to Warp Build"
```

Replace with:

```rust
            {
                #[cfg(feature = "omw_local")]
                { "" }
                #[cfg(not(feature = "omw_local"))]
                { "Welcome to Warp Build" }
            }
```

- [ ] **Step 5: Build under omw_local**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected: `Finished` with no errors.

If the auth_view_body match arm gating triggers exhaustiveness errors, switch that one file to the inside-the-arm gating shown in Step 1's fallback.

- [ ] **Step 6: Build under default**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: `Finished` with no errors.

- [ ] **Step 7: Grep verification**

```bash
for s in "Welcome to Warp!" "Sign up for Warp" "Welcome to Warp Build" "link your Warp account"; do
  echo "== $s =="
  grep -rn "$s" vendor/warp-stripped/app/src/ --include="*.rs" | grep -v test | grep -v "linear.app"
done
```

Expected: each string still appears once or twice — but always inside an `#[cfg(not(feature = "omw_local"))]` arm (visible by surrounding context). If any appears outside such a guard, that's a bug — re-edit.

- [ ] **Step 8: Commit**

```bash
git add vendor/warp-stripped/app/src/auth/auth_view_body.rs \
        vendor/warp-stripped/app/src/auth/needs_sso_link_view.rs \
        vendor/warp-stripped/app/src/auth/auth_override_warning_body.rs \
        vendor/warp-stripped/app/src/workspace/view/build_plan_migration_modal.rs
git commit -m "$(cat <<'EOF'
Blank dead "Warp" strings in auth + billing flows under omw_local

These surfaces (auth modals, SSO link view, build-plan migration) are
unreachable for an anonymous omw_local user. Gating the literal strings
to "" under omw_local removes the upstream brand text from the binary
without touching the unreachable code paths' structure.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Final verification, audit, and TODO update

**Files:**
- Run: `vendor/warp-stripped/scripts/audit-no-cloud.sh`
- Build: `bash scripts/build-mac-dmg.sh 0.0.2-rc.1` (full preview build)
- Modify: `TODO.md`

- [ ] **Step 1: Final dual-feature compile gate**

```bash
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss --no-default-features --features omw_local
cd vendor/warp-stripped && PROTOC=/opt/homebrew/bin/protoc cargo check -p warp --bin warp-oss
```

Expected: both `Finished` with no errors and no new warnings.

- [ ] **Step 2: Run the audit script**

```bash
cd vendor/warp-stripped && bash scripts/audit-no-cloud.sh
```

Expected: report shows zero forbidden hostnames in the resulting omw_local binary. If new strings or URLs leaked: investigate (probably a missed cfg arm in Task 4 or 10).

- [ ] **Step 3: Build the actual `.dmg`**

```bash
cd /Users/caijiaqi/Documents/GitHub/oh-my-warp && bash scripts/build-mac-dmg.sh 0.0.2-rc.1
```

Expected: produces `dist/omw-warp-oss-0.0.2-rc.1.dmg`. (Version string is provisional; adjust if you have a different release plan.) If the script doesn't accept the `-rc.1` suffix, run with `0.0.2` or whatever your release plan dictates.

- [ ] **Step 4: Manual UI smoke**

Mount the `.dmg`, install the app, then on a fresh launch (delete `~/Library/Application Support/omw.local.warpOss/` first to simulate first run), verify each surface:

| Surface | Expected |
|---|---|
| First terminal block | Inline banner reads "Welcome to omw (oh-my-warp)" with no Sign Up button (only X close) |
| Settings → Account (left sidebar, top page) | Single muted paragraph: "Standalone build — sign-in is disabled..." — no Sign up / Compare plans / Upgrade |
| Settings → About | Logo, version, "omw — oh-my-warp", description, "Acknowledgements" + warpdotdev/warp link, "Project home" + AndrewWayne/oh-my-warp link, "License" + scrollable AGPL text. Both links open in browser when clicked. |
| Help menu (macOS top bar) | Two items only: "Project on GitHub..." and "Report an Issue..." — both pointing at AndrewWayne/oh-my-warp |
| Cmd-T → Get Started | Title says "Welcome to omw" with "Open-source terminal — local build" tagline |
| First-launch | No "Warp is now open-source" modal pops up |
| Cmd-K command palette | Search for "AI" — no "Toggle Warp AI" entry |

If any surface still shows upstream "Warp" text or a `warpdotdev/warp` URL: identify which task missed the spot and fix. Re-run Steps 1–3.

If you cannot run the manual smoke (no Mac available, can't install the `.dmg`): say so explicitly per CLAUDE.md "For UI or frontend changes" guidance — don't claim verification you didn't do.

- [ ] **Step 5: Update TODO.md**

Open `TODO.md`. Find the section for the v0.0.2 preview (or add one if missing — match the existing v0.0.1 entry style). Add a checked box:

```markdown
- [x] **omw-local-preview-v0.0.2**: Strip residual signup / Warp-brand UI per [`docs/superpowers/specs/2026-05-01-strip-residual-signup-design.md`](./docs/superpowers/specs/2026-05-01-strip-residual-signup-design.md). Inline banner, About page, Help menu, Get Started tab rebuilt under `omw_local`; OpenWarpLaunchModal disabled; dead strings blanked. Done 2026-05-01.
```

(Adjust the bullet point structure to match whatever format the existing v0.0.1 entry uses in `TODO.md`. If unsure, run `grep -n "v0.0.1" TODO.md` first to see the precedent line.)

- [ ] **Step 6: Final commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
TODO: mark omw-local-preview-v0.0.2 residual-signup strip complete

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Summary report**

Print a one-paragraph summary of what was done, citing the spec link and the final commit SHA. Stop. Do not tag or push — release tagging is a separate manual step the project owner runs.

---

## Self-review checklist

Before declaring this plan complete:

- [ ] Every spec section (§4.1 A1–A5, §4.2 B1–B4, §6 non-goals respected) maps to a task here. Confirmed: A1→Task 2, A2→Task 3, A3→Task 4, A4→Task 5, A5→Task 6, B1→Task 7, B2→Task 8, B3→Task 9, B4→Task 10. Non-goals (no module rewrites, no LICENSE edits, no rename) are honored — none of the tasks edit anything outside the listed file set.
- [ ] No placeholder strings or "TODO" / "TBD" / "implement appropriately" anywhere.
- [ ] Every code step shows the exact code, not a description.
- [ ] Every build step has an exact command and expected output.
- [ ] Brand-rule (CLAUDE.md §5): no new occurrences of capitalized `Warp` in product copy. Confirmed by grep instructions in Tasks 4, 5, 6, 10.
- [ ] Fork-strategy (`specs/fork-strategy.md` §2): every cfg gate is reversible (default branch preserves upstream behavior). Confirmed by reverse-build step in every code task.
- [ ] Type / function name consistency: `render_anonymous_account_info` keeps the same signature in both arms; `make_new_help_menu` returns `Menu`; `AboutPageWidget` keeps the same fields under default and adds three under omw_local; the new `LICENSE_TEXT` constant uses the verified path `../../../../../LICENSE`.
