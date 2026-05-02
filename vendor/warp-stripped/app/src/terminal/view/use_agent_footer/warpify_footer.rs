use std::sync::Arc;

use parking_lot::FairMutex;
use warpui::prelude::Empty;
use warpui::{
    elements::{
        ChildView, Container, CrossAxisAlignment, Expanded, Flex, MainAxisSize, ParentElement,
    },
    AppContext, Element, Entity, TypedActionView, View, ViewContext, ViewHandle,
};

#[cfg(feature = "omw_local")]
use warpui::EntityId;

use crate::{
    channel::ChannelState,
    terminal::view::{TerminalModel, PADDING_LEFT},
    ui_components::icons::Icon,
    view_components::action_button::{ActionButton, ButtonSize, KeystrokeSource, TooltipAlignment},
};

use super::{AgentFooterButtonTheme, USE_AGENT_KEYSTROKE};
use crate::terminal::view::block_banner::WarpificationMode;

/// Footer view rendered for detected subshell/SSH commands, offering Warpify and,
/// when official cloud services are enabled, an agent handoff button.
///
/// Wiring 5 also threads an "Remote Control" button through this view,
/// gated on the `omw_local` feature, that toggles the embedded `omw-remote`
/// daemon. We dock it on warpify_footer for the wiring pass because the
/// upstream agent_input_footer toolbar is configurable and adding a new kind
/// there would require touching files outside the wiring scope.
pub(super) struct WarpifyFooterView {
    terminal_model: Arc<FairMutex<TerminalModel>>,
    warpify_button: ViewHandle<ActionButton>,
    use_agent_button: ViewHandle<ActionButton>,
    dismiss_button: ViewHandle<ActionButton>,
    #[cfg(feature = "omw_local")]
    omw_pair_button: ViewHandle<ActionButton>,
    /// Owning `TerminalView`'s id, used to key per-pane share state on the
    /// Phone button (so the label reflects whether THIS pane is shared, not
    /// just whether the daemon is up). v0.4-thin multi-pane share.
    #[cfg(feature = "omw_local")]
    terminal_view_id: EntityId,
    mode: Option<WarpificationMode>,
}

impl WarpifyFooterView {
    pub fn new(
        terminal_model: Arc<FairMutex<TerminalModel>>,
        #[cfg(feature = "omw_local")] terminal_view_id: EntityId,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let button_size = ButtonSize::XSmall;

        let warpify_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Warpify subshell", AgentFooterButtonTheme::new(None))
                .with_icon(Icon::Warp)
                .with_size(button_size)
                .with_tooltip("Enable Warp shell integration in this session")
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(WarpifyFooterViewAction::Warpify);
                })
        });

        let use_agent_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("Use agent", AgentFooterButtonTheme::new(None))
                .with_icon(Icon::Oz)
                .with_keybinding(KeystrokeSource::Fixed(USE_AGENT_KEYSTROKE.clone()), ctx)
                .with_size(button_size)
                .with_tooltip("Ask the Warp agent to assist")
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(WarpifyFooterViewAction::UseAgent);
                })
        });

        let dismiss_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Dismiss", AgentFooterButtonTheme::new(None))
                .with_size(button_size)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(WarpifyFooterViewAction::Dismiss);
                })
        });

        #[cfg(feature = "omw_local")]
        let omw_pair_button = ctx.add_typed_action_view(|_ctx| {
            // Initial paint reads the live (status, this-pane-shared) pair so
            // a hot-reloaded view that lands mid-share renders the right label
            // without waiting for the next watch tick.
            let state = crate::omw::OmwRemoteState::shared();
            let initial_status = state.status();
            let initial_shared = state.is_pane_shared(terminal_view_id);
            let (label, tooltip) =
                crate::omw::pair_button::pair_button_text(&initial_status, initial_shared);
            ActionButton::new(label, AgentFooterButtonTheme::new(None))
                .with_icon(Icon::Phone)
                .with_size(button_size)
                .with_tooltip(tooltip)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(WarpifyFooterViewAction::ToggleOmwPair);
                })
        });
        // v0.4-thin: subscribe to BOTH the daemon-status watch AND the
        // share-map watch so the (label, tooltip) tracks per-pane share state,
        // not just the global daemon state.
        #[cfg(feature = "omw_local")]
        let omw_status_stream =
            crate::omw::OmwRemoteState::shared().subscribe_status_stream();
        #[cfg(feature = "omw_local")]
        let omw_share_stream = crate::omw::OmwRemoteState::shared().subscribe_share_stream();

        let me = Self {
            terminal_model,
            warpify_button,
            use_agent_button,
            dismiss_button,
            #[cfg(feature = "omw_local")]
            omw_pair_button,
            #[cfg(feature = "omw_local")]
            terminal_view_id,
            mode: None,
        };

        #[cfg(feature = "omw_local")]
        match omw_status_stream {
            Ok(stream) => {
                ctx.spawn_stream_local(
                    stream,
                    |me, _status: crate::omw::OmwRemoteStatus, ctx| {
                        me.sync_omw_pair_button(ctx);
                    },
                    |_, _| {},
                );
            }
            Err(e) => {
                log::warn!(
                    "omw-remote: warpify-footer Phone button status-stream subscribe failed: {e}"
                );
            }
        }
        #[cfg(feature = "omw_local")]
        match omw_share_stream {
            Ok(stream) => {
                ctx.spawn_stream_local(
                    stream,
                    |me, _tick: u64, ctx| {
                        me.sync_omw_pair_button(ctx);
                    },
                    |_, _| {},
                );
            }
            Err(e) => {
                log::warn!(
                    "omw-remote: warpify-footer Phone button share-stream subscribe failed: {e}"
                );
            }
        }

        me
    }

    /// v0.4-thin: refresh the omw Phone button's label and tooltip from the
    /// LIVE pair of (daemon status, this-pane-shared). Mirrors the agent
    /// input footer's `sync_omw_pair_button`.
    #[cfg(feature = "omw_local")]
    fn sync_omw_pair_button(&self, ctx: &mut ViewContext<Self>) {
        let state = crate::omw::OmwRemoteState::shared();
        let status = state.status();
        let is_shared = state.is_pane_shared(self.terminal_view_id);
        let (label, tooltip) = crate::omw::pair_button::pair_button_text(&status, is_shared);
        let active = matches!(status, crate::omw::OmwRemoteStatus::Running { .. }) && is_shared;
        self.omw_pair_button.update(ctx, |button, ctx| {
            button.set_label(label, ctx);
            button.set_tooltip(Some(tooltip), ctx);
            button.set_active(active, ctx);
        });
    }

    /// Updates the warpify button label, keybinding, and stores the current warpification mode.
    pub fn set_mode(&mut self, mode: WarpificationMode, ctx: &mut ViewContext<Self>) {
        let (label, binding_name) = match mode {
            WarpificationMode::Ssh { .. } => {
                ("Warpify SSH session", "terminal:warpify_ssh_session")
            }
            WarpificationMode::Subshell { .. } => ("Warpify subshell", "terminal:warpify_subshell"),
        };
        self.warpify_button.update(ctx, |button, ctx| {
            button.set_label(label, ctx);
            button.set_keybinding(Some(KeystrokeSource::Binding(binding_name)), ctx);
        });
        self.mode = Some(mode);
        ctx.notify();
    }

    /// Returns the current warpification mode, if set.
    pub fn mode(&self) -> Option<&WarpificationMode> {
        self.mode.as_ref()
    }

    /// Clears the warpification mode.
    pub fn clear_mode(&mut self, ctx: &mut ViewContext<Self>) {
        self.mode = None;
        self.warpify_button.update(ctx, |button, ctx| {
            button.set_keybinding(None, ctx);
        });
        ctx.notify();
    }
}

#[derive(Debug, Clone)]
pub enum WarpifyFooterViewAction {
    Warpify,
    UseAgent,
    Dismiss,
    /// Toggle the embedded `omw-remote` daemon (Wiring 5).
    #[cfg(feature = "omw_local")]
    ToggleOmwPair,
}

pub enum WarpifyFooterViewEvent {
    Warpify { mode: WarpificationMode },
    UseAgent,
    Dismiss,
    /// Re-emitted when the user clicks the Remote Control button. The parent
    /// `UseAgentToolbar` translates this into `UseAgentToolbarEvent::ToggleOmwPair`.
    #[cfg(feature = "omw_local")]
    ToggleOmwPair,
}

impl Entity for WarpifyFooterView {
    type Event = WarpifyFooterViewEvent;
}

impl View for WarpifyFooterView {
    fn ui_name() -> &'static str {
        "WarpifyFooterView"
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        let terminal_model = self.terminal_model.lock();

        let mut button_row = Flex::row()
            .with_spacing(4.)
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(ChildView::new(&self.warpify_button).finish());
        if ChannelState::official_cloud_services_enabled() {
            button_row = button_row.with_child(ChildView::new(&self.use_agent_button).finish());
        }
        #[cfg(feature = "omw_local")]
        {
            button_row = button_row.with_child(ChildView::new(&self.omw_pair_button).finish());
        }
        let button_row = button_row
            .with_child(Expanded::new(1., Empty::new().finish()).finish())
            .with_child(ChildView::new(&self.dismiss_button).finish());

        let mut container = Container::new(button_row.finish())
            .with_horizontal_padding(*PADDING_LEFT)
            .with_vertical_padding(4.);

        if terminal_model.is_alt_screen_active() {
            if let Some(bg_color) = terminal_model.alt_screen().inferred_bg_color() {
                container = container.with_background(bg_color);
            }
        }

        container.finish()
    }
}

impl TypedActionView for WarpifyFooterView {
    type Action = WarpifyFooterViewAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            WarpifyFooterViewAction::Warpify => {
                if let Some(mode) = self.mode.clone() {
                    self.clear_mode(ctx);
                    ctx.emit(WarpifyFooterViewEvent::Warpify { mode });
                }
            }
            WarpifyFooterViewAction::UseAgent => {
                if !ChannelState::official_cloud_services_enabled() {
                    return;
                }
                self.clear_mode(ctx);
                ctx.emit(WarpifyFooterViewEvent::UseAgent);
            }
            WarpifyFooterViewAction::Dismiss => {
                self.clear_mode(ctx);
                ctx.emit(WarpifyFooterViewEvent::Dismiss);
            }
            #[cfg(feature = "omw_local")]
            WarpifyFooterViewAction::ToggleOmwPair => {
                ctx.emit(WarpifyFooterViewEvent::ToggleOmwPair);
            }
        }
    }
}
