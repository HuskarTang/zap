use settings::Setting;
use warp_core::ui::color::blend::Blend;
use warp_core::ui::theme::Fill as ThemeFill;
use warpui::{
    elements::{
        Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Expanded,
        Flex, MainAxisSize, MouseStateHandle, ParentElement, Radius, Text,
    },
    fonts::{Properties, Weight},
    ui_components::{
        button::ButtonVariant,
        components::{Coords, UiComponent, UiComponentStyles},
    },
    AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use super::settings_page::{
    render_page_title, render_sub_header_with_description, MatchData, PageType, SettingsPageMeta,
    SettingsPageViewHandle, SettingsWidget,
};
use super::SettingsSection;
use crate::ai::agent_providers::fetch_openai_compatible_models;
use crate::appearance::Appearance;
use crate::settings::{normalize_server_url, AISettings};
use crate::sub2api::{
    client::Sub2ApiKey,
    import::{
        key_import_state, merge_provider_models, ImportIntoSettingsContext, Sub2ApiKeyImportState,
    },
    Sub2ApiBootstrap, Sub2ApiClient, Sub2ApiProfile, Sub2ApiSessionEvent, Sub2ApiSessionStore,
};

const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_PADDING_X: f32 = 12.0;
const BUTTON_PADDING_Y: f32 = 6.0;
const CARD_RADIUS: f32 = 4.0;
const CARD_PADDING_X: f32 = 14.0;
const CARD_PADDING_Y: f32 = 12.0;
const CARD_GAP: f32 = 12.0;
const KEY_ROW_PADDING_Y: f32 = 10.0;
const STATUS_BADGE_RADIUS: f32 = 3.0;
const STATUS_BADGE_PADDING_X: f32 = 8.0;
const STATUS_BADGE_PADDING_Y: f32 = 4.0;
const LABEL_WIDTH: f32 = 72.0;

#[derive(Debug, Clone)]
pub enum AccountPageAction {
    LoginResult(Result<Sub2ApiBootstrap, String>),
    Refresh,
    BootstrapResult(Result<Sub2ApiBootstrap, String>),
    ImportAll,
    Logout,
    Imported(String),
}

#[derive(Debug, Clone, Default)]
enum AccountPageState {
    #[default]
    Idle,
    Loading,
    Loaded(Sub2ApiBootstrap),
    Error(String),
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountPageEvent {
    LoggedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountKeyImportState {
    Imported,
    Importable,
    Unavailable,
}

pub struct AccountPageView {
    page: PageType<Self>,
    refresh_button_state: MouseStateHandle,
    import_all_button_state: MouseStateHandle,
    logout_button_state: MouseStateHandle,
    state: AccountPageState,
}

impl AccountPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        ctx.subscribe_to_model(&Sub2ApiSessionStore::handle(ctx), |me, _, event, ctx| {
            me.handle_session_event(event, ctx);
        });

        let mut view = Self {
            page: PageType::new_monolith(AccountPageWidget::default(), None, false),
            refresh_button_state: MouseStateHandle::default(),
            import_all_button_state: MouseStateHandle::default(),
            logout_button_state: MouseStateHandle::default(),
            state: AccountPageState::Idle,
        };
        if Sub2ApiSessionStore::as_ref(ctx).is_logged_in() {
            view.spawn_bootstrap(ctx, false);
        }
        view
    }

    fn handle_session_event(&mut self, event: &Sub2ApiSessionEvent, ctx: &mut ViewContext<Self>) {
        match event {
            Sub2ApiSessionEvent::SessionChanged => {
                if Sub2ApiSessionStore::as_ref(ctx).is_logged_in() {
                    self.spawn_bootstrap(ctx, true);
                } else {
                    self.state = AccountPageState::Idle;
                    ctx.notify();
                }
            }
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        Self {
            page: PageType::new_monolith(AccountPageWidget::default(), None, false),
            refresh_button_state: MouseStateHandle::default(),
            import_all_button_state: MouseStateHandle::default(),
            logout_button_state: MouseStateHandle::default(),
            state: AccountPageState::Idle,
        }
    }

    fn current_server_url(&self, ctx: &AppContext) -> String {
        let session_url = Sub2ApiSessionStore::as_ref(ctx)
            .server_url()
            .unwrap_or(crate::settings::DEFAULT_SERVER_URL);
        normalize_server_url(session_url)
    }

    fn spawn_bootstrap(&mut self, ctx: &mut ViewContext<Self>, as_login: bool) {
        let Some(token) = Sub2ApiSessionStore::as_ref(ctx)
            .access_token()
            .map(str::to_owned)
        else {
            self.state = AccountPageState::Idle;
            return;
        };
        let server_url = self.current_server_url(ctx);
        self.state = AccountPageState::Loading;
        ctx.notify();
        ctx.spawn(
            async move {
                Sub2ApiClient::new(server_url)
                    .bootstrap(&token)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |view, result, ctx| {
                let action = if as_login {
                    AccountPageAction::LoginResult(result)
                } else {
                    AccountPageAction::BootstrapResult(result)
                };
                view.handle_action(&action, ctx);
            },
        );
    }

    fn render_button(
        label: impl Into<String>,
        state: MouseStateHandle,
        variant: ButtonVariant,
        action: AccountPageAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        appearance
            .ui_builder()
            .button(variant, state)
            .with_style(UiComponentStyles {
                height: Some(BUTTON_HEIGHT),
                font_size: Some(appearance.ui_font_body()),
                padding: Some(
                    Coords::uniform(BUTTON_PADDING_Y)
                        .left(BUTTON_PADDING_X)
                        .right(BUTTON_PADDING_X),
                ),
                ..Default::default()
            })
            .with_centered_text_label(label.into())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(action.clone());
            })
            .finish()
    }
}

impl Entity for AccountPageView {
    type Event = AccountPageEvent;
}

impl TypedActionView for AccountPageView {
    type Action = AccountPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            AccountPageAction::LoginResult(result) | AccountPageAction::BootstrapResult(result) => {
                match result {
                    Ok(bootstrap) => {
                        self.state = AccountPageState::Loaded(bootstrap.clone());
                    }
                    Err(err) => {
                        self.state = AccountPageState::Error(err.clone());
                    }
                }
                ctx.notify();
            }
            AccountPageAction::Refresh => self.spawn_bootstrap(ctx, false),
            AccountPageAction::ImportAll => {
                if let AccountPageState::Loaded(bootstrap) = &self.state {
                    let report = ctx.import_sub2api_bootstrap(bootstrap);
                    let imported_provider_ids = report.provider_ids.clone();
                    let providers = AISettings::as_ref(ctx).agent_providers.value().clone();
                    let provider_fetch_inputs: Vec<(String, String, Option<String>)> =
                        imported_provider_ids
                            .iter()
                            .filter_map(|provider_id| {
                                let provider = providers
                                    .iter()
                                    .find(|provider| provider.id == *provider_id)?;
                                let api_key =
                                    crate::ai::agent_providers::AgentProviderSecrets::as_ref(ctx)
                                        .get(provider_id)
                                        .map(str::to_owned);
                                Some((provider_id.clone(), provider.base_url.clone(), api_key))
                            })
                            .collect();
                    self.state = AccountPageState::Loading;
                    ctx.notify();
                    ctx.spawn(
                        async move {
                            let mut fetched_by_provider = Vec::new();
                            for (provider_id, base_url, api_key) in provider_fetch_inputs {
                                let fetched = fetch_openai_compatible_models(
                                    http_client::Client::new(),
                                    &base_url,
                                    api_key.as_deref(),
                                )
                                .await
                                .unwrap_or_else(|err| {
                                    log::warn!(
                                        "sub2api import model sync failed for provider {}: {}",
                                        provider_id,
                                        err
                                    );
                                    Vec::new()
                                });
                                fetched_by_provider.push((provider_id, fetched));
                            }
                            (report, fetched_by_provider)
                        },
                        move |view, (report, fetched_by_provider), ctx| {
                            let added_models =
                                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                                    let mut providers = settings.agent_providers.value().clone();
                                    let mut total_added = 0usize;
                                    for (provider_id, fetched) in fetched_by_provider {
                                        let Some(provider) = providers
                                            .iter_mut()
                                            .find(|provider| provider.id == provider_id)
                                        else {
                                            continue;
                                        };
                                        if fetched.is_empty() {
                                            continue;
                                        }
                                        total_added += merge_provider_models(provider, fetched);
                                    }
                                    let _ = settings.agent_providers.set_value(providers, ctx);
                                    total_added
                                });

                            view.state = AccountPageState::Message(format!(
                                "已导入 {} 个 sub2api provider，新增同步 {} 个模型",
                                report.imported, added_models
                            ));
                            ctx.notify();
                        },
                    );
                }
            }
            AccountPageAction::Logout => {
                Sub2ApiSessionStore::handle(ctx).update(ctx, |store, ctx| store.clear(ctx));
                self.state = AccountPageState::Idle;
                ctx.emit(AccountPageEvent::LoggedOut);
                ctx.notify();
            }
            AccountPageAction::Imported(message) => {
                self.state = AccountPageState::Message(message.clone());
                ctx.notify();
            }
        }
    }
}

impl View for AccountPageView {
    fn ui_name() -> &'static str {
        "AccountPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl SettingsPageMeta for AccountPageView {
    fn section() -> SettingsSection {
        SettingsSection::Account
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id);
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<AccountPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<AccountPageView>) -> Self {
        SettingsPageViewHandle::Account(view_handle)
    }
}

#[derive(Default)]
struct AccountPageWidget;

impl SettingsWidget for AccountPageWidget {
    type View = AccountPageView;

    fn search_terms(&self) -> &str {
        "account sub2api login api key provider import 账户 登录"
    }

    fn render(
        &self,
        view: &AccountPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let mut content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(render_page_title("账户", appearance))
            .with_child(render_sub_header_with_description(
                appearance,
                "服务器账户",
                "当前登录服务器返回的用户资料、网关地址和可用 API Key。",
            ));

        match &view.state {
            AccountPageState::Loaded(bootstrap) => {
                content.add_child(render_profile_card(bootstrap, appearance));
                content.add_child(render_keys_section(bootstrap, appearance, app));
                content.add_child(render_actions(view, appearance));
            }
            AccountPageState::Loading => {
                content.add_child(render_feedback_card("加载中...", None, appearance));
            }
            AccountPageState::Error(err) => {
                content.add_child(render_feedback_card(
                    "账户信息获取失败",
                    Some(err.as_str()),
                    appearance,
                ));
                content.add_child(render_actions(view, appearance));
            }
            AccountPageState::Message(message) => {
                content.add_child(render_feedback_card(message, None, appearance));
                content.add_child(render_actions(view, appearance));
            }
            AccountPageState::Idle => {
                if Sub2ApiSessionStore::as_ref(app).is_logged_in() {
                    content.add_child(render_feedback_card(
                        "准备加载账户信息...",
                        None,
                        appearance,
                    ));
                    content.add_child(render_actions(view, appearance));
                } else {
                    content.add_child(render_feedback_card(
                        "未登录，请在弹窗中登录。",
                        None,
                        appearance,
                    ));
                }
            }
        }

        content.finish()
    }
}

fn render_actions(view: &AccountPageView, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Container::new(AccountPageView::render_button(
                    "刷新",
                    view.refresh_button_state.clone(),
                    ButtonVariant::Basic,
                    AccountPageAction::Refresh,
                    appearance,
                ))
                .with_margin_right(8.)
                .finish(),
            )
            .with_child(
                Container::new(AccountPageView::render_button(
                    "全部导入",
                    view.import_all_button_state.clone(),
                    ButtonVariant::Accent,
                    AccountPageAction::ImportAll,
                    appearance,
                ))
                .with_margin_right(8.)
                .finish(),
            )
            .with_child(AccountPageView::render_button(
                "登出",
                view.logout_button_state.clone(),
                ButtonVariant::Secondary,
                AccountPageAction::Logout,
                appearance,
            ))
            .finish(),
    )
    .with_margin_top(8.)
    .finish()
}

fn render_profile_card(bootstrap: &Sub2ApiBootstrap, appearance: &Appearance) -> Box<dyn Element> {
    let Sub2ApiProfile {
        email,
        username,
        balance,
        concurrency,
        status,
    } = &bootstrap.profile;
    let username = if username.trim().is_empty() {
        "未设置"
    } else {
        username
    };

    render_card(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(render_profile_row("邮箱", email, appearance))
            .with_child(render_profile_row("用户名", username, appearance))
            .with_child(render_profile_row(
                "余额",
                &format!("{balance:.2}"),
                appearance,
            ))
            .with_child(render_profile_row(
                "并发",
                &concurrency.to_string(),
                appearance,
            ))
            .with_child(render_profile_row("状态", status, appearance))
            .with_child(render_profile_row(
                "Gateway",
                &bootstrap.api_base_url,
                appearance,
            ))
            .finish(),
        appearance,
    )
}

fn render_keys_section(
    bootstrap: &Sub2ApiBootstrap,
    appearance: &Appearance,
    app: &AppContext,
) -> Box<dyn Element> {
    let providers = AISettings::as_ref(app).agent_providers.value().clone();

    let mut list = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    list.add_child(render_sub_header_with_description(
        appearance,
        "API Keys",
        "可导入到 Agent 提供商配置的 active key。",
    ));

    if bootstrap.keys.is_empty() {
        list.add_child(render_card(
            Container::new(sub_text("当前账户没有可展示的 API Key。", appearance)).finish(),
            appearance,
        ));
        return list.finish();
    }

    let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for (index, key) in bootstrap.keys.iter().enumerate() {
        rows.add_child(render_key_row(
            &bootstrap.api_base_url,
            key,
            &providers,
            appearance,
            index + 1 < bootstrap.keys.len(),
        ));
    }

    list.add_child(render_card(rows.finish(), appearance));
    list.finish()
}

fn render_key_row(
    api_base_url: &str,
    key: &Sub2ApiKey,
    providers: &[crate::settings::AgentProvider],
    appearance: &Appearance,
    with_divider: bool,
) -> Box<dyn Element> {
    let import_state = match key_import_state(api_base_url, key, providers) {
        Sub2ApiKeyImportState::Imported => AccountKeyImportState::Imported,
        Sub2ApiKeyImportState::Importable => AccountKeyImportState::Importable,
        Sub2ApiKeyImportState::Unavailable => AccountKeyImportState::Unavailable,
    };
    let platform = key
        .group
        .as_ref()
        .and_then(|group| group.platform.as_deref())
        .unwrap_or("openai");
    let group_name = key
        .group
        .as_ref()
        .and_then(|group| group.name.as_deref())
        .filter(|name: &&str| !name.trim().is_empty())
        .unwrap_or("未分组");
    let key_name = if key.name.trim().is_empty() {
        "未命名 Key"
    } else {
        key.name.as_str()
    };
    let status = if key.status.trim().is_empty() {
        "unknown"
    } else {
        key.status.as_str()
    };
    let model_count = key.models.len();

    let mut container = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(
                Expanded::new(
                    1.,
                    ConstrainedBox::new(
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_child(text(
                                format!("{key_name} · {status} · {platform} · {model_count} 模型"),
                                appearance,
                            ))
                            .with_child(
                                Container::new(sub_text(
                                    format!("Key {} · 分组 {group_name}", masked_secret(&key.key)),
                                    appearance,
                                ))
                                .with_margin_top(4.)
                                .finish(),
                            )
                            .finish(),
                    )
                    .with_min_width(280.)
                    .finish(),
                )
                .finish(),
            )
            .with_child(
                Container::new(status_badge(import_state, appearance))
                    .with_margin_left(12.)
                    .finish(),
            )
            .finish(),
    )
    .with_padding_top(KEY_ROW_PADDING_Y)
    .with_padding_bottom(KEY_ROW_PADDING_Y);

    if with_divider {
        container = container
            .with_border(Border::bottom(1.).with_border_fill(appearance.theme().outline()));
    }

    container.finish()
}

fn render_profile_row(label: &str, value: &str, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(
                ConstrainedBox::new(sub_text(label, appearance))
                    .with_min_width(LABEL_WIDTH)
                    .finish(),
            )
            .with_child(text(value.to_owned(), appearance))
            .finish(),
    )
    .with_margin_bottom(8.)
    .finish()
}

fn render_feedback_card(
    title: impl Into<String>,
    detail: Option<&str>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Start);
    column.add_child(text(title.into(), appearance));
    if let Some(detail) = detail.filter(|detail| !detail.trim().is_empty()) {
        column.add_child(
            Container::new(sub_text(detail.to_owned(), appearance))
                .with_margin_top(6.)
                .finish(),
        );
    }
    render_card(column.finish(), appearance)
}

fn render_card(child: Box<dyn Element>, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(child)
        .with_background(appearance.theme().surface_2())
        .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(CARD_RADIUS)))
        .with_horizontal_padding(CARD_PADDING_X)
        .with_vertical_padding(CARD_PADDING_Y)
        .with_margin_bottom(CARD_GAP)
        .finish()
}

fn status_badge(state: AccountKeyImportState, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let (label, background, border) = match state {
        AccountKeyImportState::Imported => (
            "已导入",
            theme
                .surface_2()
                .blend(&ThemeFill::Solid(theme.ui_green_color()).with_opacity(18))
                .into_solid(),
            ThemeFill::Solid(theme.ui_green_color())
                .with_opacity(40)
                .into_solid(),
        ),
        AccountKeyImportState::Importable => (
            "可导入",
            theme
                .surface_2()
                .blend(&theme.accent().with_opacity(18))
                .into_solid(),
            theme.accent().with_opacity(40).into_solid(),
        ),
        AccountKeyImportState::Unavailable => (
            "不可导入",
            theme
                .surface_2()
                .blend(&ThemeFill::Solid(theme.ui_warning_color()).with_opacity(18))
                .into_solid(),
            ThemeFill::Solid(theme.ui_warning_color())
                .with_opacity(40)
                .into_solid(),
        ),
    };

    Container::new(
        Text::new(
            label,
            appearance.ui_font_family(),
            appearance.ui_font_footnote(),
        )
        .with_color(theme.main_text_color(theme.surface_2()).into())
        .with_style(Properties::default().weight(Weight::Semibold))
        .finish(),
    )
    .with_background(background)
    .with_border(Border::all(1.).with_border_fill(border))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(STATUS_BADGE_RADIUS)))
    .with_padding_left(STATUS_BADGE_PADDING_X)
    .with_padding_right(STATUS_BADGE_PADDING_X)
    .with_padding_top(STATUS_BADGE_PADDING_Y)
    .with_padding_bottom(STATUS_BADGE_PADDING_Y)
    .finish()
}

fn masked_secret(secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return "<empty>".to_owned();
    }

    let suffix_chars: Vec<char> = secret.chars().rev().take(4).collect();
    let suffix: String = suffix_chars.into_iter().rev().collect();
    if secret.chars().count() <= 4 {
        format!("••••{suffix}")
    } else {
        format!("••••{suffix}")
    }
}

fn text(text: impl Into<String>, appearance: &Appearance) -> Box<dyn Element> {
    Text::new(
        text.into(),
        appearance.ui_font_family(),
        appearance.ui_font_size(),
    )
    .with_color(appearance.theme().active_ui_text_color().into())
    .with_style(Properties::default().weight(Weight::Medium))
    .finish()
}

fn sub_text(text: impl Into<String>, appearance: &Appearance) -> Box<dyn Element> {
    Text::new(
        text.into(),
        appearance.ui_font_family(),
        appearance.ui_font_body(),
    )
    .with_color(appearance.theme().nonactive_ui_text_color().into())
    .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_secret_keeps_last_four_characters() {
        assert_eq!(masked_secret("sk-abc1234"), "••••1234");
        assert_eq!(masked_secret("123"), "••••123");
        assert_eq!(masked_secret(""), "<empty>");
    }

    #[test]
    fn new_for_test_starts_idle() {
        let view = AccountPageView::new_for_test();
        assert!(matches!(view.state, AccountPageState::Idle));
    }
}
