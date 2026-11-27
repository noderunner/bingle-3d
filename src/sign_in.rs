//! Signing in to an account on the server, and out again.
//!
//! [`Form`] is the sign-in with no Bevy in it: the handle and the password, then a code when the
//! account has a second factor. [`fields`] draws its fields on the login screen (`login.rs`),
//! and [`status`] says under the main screen's server status who the client is.
//!
//! The handle and the code are Bevy's own text inputs. Bevy cannot hide what is typed in one yet,
//! so the password has a field of its own ([`PasswordField`]): it shows a dot for each character,
//! takes typing, Backspace and pasting (which password managers need), and has no cursor to move
//! (one blinks after the dots while it has the keyboard, as Bevy's blinks in the others).
//!
//! Every field's edge is lit while the pointer is over it, and more while it has the keyboard
//! ([`edge`]).
//! The password is never logged (`Form`'s `Debug` leaves it out), and is cleared once sent.

use bevy::clipboard::{Clipboard, ClipboardRead};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{AutoFocus, FocusCause, FocusedInput, InputFocus};
use bevy::picking::Pickable;
use bevy::picking::events::PointerPress;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};
use bevy::ui_widgets::TextInput;

use crate::browser::{Action, button, buttons, colors, line};
use crate::fonts::UiFonts;
use crate::net::session::{Account, Code, Problem, SignIn};
use crate::net::{Net, Request};

pub struct SignInPlugin;

impl Plugin for SignInPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pasting>().add_systems(
            Update,
            (keep_typed, paste_password, show_dots, outline_fields),
        );
    }
}

/// Signing in and out, for the panels to draw.
#[derive(Resource, Debug, Default)]
pub struct Signing {
    /// The sign-in form, while it is open.
    pub form: Option<Form>,
    /// Why the server could not be told of the last sign-out (the session is forgotten here all
    /// the same).
    pub out_problem: Option<Problem>,
}

#[derive(Clone, Default, PartialEq)]
pub struct Form {
    pub handle: String,
    password: String,
    /// The code from the account's authenticator app, or a recovery code.
    pub code: String,
    pub step: Step,
    /// The code asked for is one of the account's recovery codes.
    pub recovery: bool,
    /// Sent, and not answered yet.
    pub busy: bool,
    /// Why the last step was turned down.
    pub problem: Option<Problem>,
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Form")
            .field("handle", &self.handle)
            .field(
                "password",
                &format_args!("({} characters)", self.password_len()),
            )
            .field("step", &self.step)
            .field("recovery", &self.recovery)
            .field("busy", &self.busy)
            .field("problem", &self.problem)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Step {
    /// The handle and the password.
    #[default]
    Password,
    /// A code, after the password was right; `pending` goes back with it.
    Code { pending: String },
}

impl Form {
    pub fn password_len(&self) -> usize {
        self.password.chars().count()
    }

    /// Typed or pasted into the password: printable characters only.
    pub fn type_password(&mut self, text: &str) {
        self.password
            .extend(text.chars().filter(|c| !c.is_control()));
    }

    pub fn erase_password(&mut self) {
        self.password.pop();
    }

    /// What to send for the step the form is at, if it is filled in and not already sent. The
    /// password goes with the request, and is no longer kept.
    pub fn request(&mut self) -> Option<Request> {
        if self.busy {
            return None;
        }
        let request = match &self.step {
            Step::Password => {
                let handle = self.handle.trim();
                if handle.is_empty() || self.password.is_empty() {
                    self.problem = None;
                    return None;
                }
                Request::SignIn {
                    handle: handle.to_string(),
                    password: std::mem::take(&mut self.password),
                }
            }
            Step::Code { pending } => {
                let code = self.code.trim();
                if code.is_empty() {
                    return None;
                }
                let code = if self.recovery {
                    Code::Recovery(code.to_string())
                } else {
                    Code::App(code.to_string())
                };
                Request::SecondFactor {
                    pending: pending.clone(),
                    code,
                }
            }
        };
        self.busy = true;
        self.problem = None;
        Some(request)
    }
}

impl Signing {
    /// Opens the form, empty but for the handle last used.
    pub fn open(&mut self, handle: &str) {
        self.form = Some(Form {
            handle: handle.to_string(),
            ..Form::default()
        });
        self.out_problem = None;
    }

    /// The server answered a step of the sign-in. Signed in, the form closes; asked for a code,
    /// it asks for one; turned down, it says why.
    pub fn answered(&mut self, result: Result<SignIn, Problem>) -> Option<SignIn> {
        let form = self.form.as_mut()?;
        form.busy = false;
        match result {
            Ok(SignIn::Done(account)) => {
                self.form = None;
                Some(SignIn::Done(account))
            }
            Ok(SignIn::NeedsCode { pending }) => {
                form.step = Step::Code {
                    pending: pending.clone(),
                };
                form.code.clear();
                form.problem = None;
                Some(SignIn::NeedsCode { pending })
            }
            Err(problem) => {
                form.code.clear();
                form.problem = Some(problem);
                None
            }
        }
    }

    pub fn signed_out(&mut self, result: Result<(), Problem>) {
        self.out_problem = result.err();
    }
}

/// Sends the step the form is at, if it is filled in.
pub fn submit(net: &Net, signing: &mut Signing) {
    if let Some(request) = signing.form.as_mut().and_then(Form::request) {
        net.send(request);
    }
}

/// Who the client is on this server, under the server's status: the account, with *Sign out*,
/// or a guest, with *Sign in*.
pub fn status(
    area: &mut ChildSpawnerCommands,
    f: &UiFonts,
    account: Option<&Account>,
    session_ended: bool,
    signing: &Signing,
) {
    if session_ended {
        area.spawn(line(
            f.regular(13.0),
            colors::WARN,
            "You were signed out: the server ended the session.",
        ));
    }
    if let Some(problem) = &signing.out_problem {
        area.spawn(line(
            f.regular(13.0),
            colors::WARN,
            &format!(
                "Signed out here, but the server could not be told: {}",
                problem.message()
            ),
        ));
    }
    area.spawn(buttons()).with_children(|row| match account {
        Some(account) => {
            let who = if account.name() == account.handle {
                account.handle.clone()
            } else {
                format!("{} ({})", account.name(), account.handle)
            };
            row.spawn(line(
                f.regular(14.0),
                colors::TEXT,
                &format!("Signed in as {who}"),
            ));
            row.spawn(button(f, "Sign out", Action::SignOut));
        }
        None => {
            row.spawn(line(f.regular(14.0), colors::MUTED, "Playing as a guest"));
            if signing.form.is_none() {
                row.spawn(button(f, "Sign in", Action::OpenSignIn));
            }
        }
    });
    if account.is_some_and(|a| a.must_change_password) {
        area.spawn(line(
            f.regular(13.0),
            colors::WARN,
            "This account must change its password, on the server's web page, before it can \
             play.",
        ));
    }
}

/// The handle's field.
#[derive(Component)]
pub struct HandleField;

/// The code's field.
#[derive(Component)]
pub struct CodeField;

/// The password's field: a dot for each character typed.
#[derive(Component)]
pub struct PasswordField;

/// The dots in the password's field.
#[derive(Component)]
struct Dots;

/// A field of the form, outlined while it has the keyboard. A headset's keyboard stands
/// before the player while one has it (`vr_keyboard.rs`).
#[derive(Component)]
pub(crate) struct Field;

/// The form's fields, on the login screen: the handle and the password (tab stops `first` and
/// the next), or the code once the password was right. Enter in any of them sends the form.
pub fn fields(area: &mut ChildSpawnerCommands, f: &UiFonts, form: &Form, first: i32) {
    area.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        flex_shrink: 0.0,
        ..default()
    })
    .observe(on_form_key)
    .with_children(|area| match &form.step {
        Step::Password => {
            area.spawn(line(f.regular(14.0), colors::TEXT, "Username"));
            let mut handle =
                area.spawn((HandleField, text_field(f, &form.handle), TabIndex(first)));
            if form.handle.is_empty() {
                handle.insert(AutoFocus);
            }
            area.spawn(line(f.regular(14.0), colors::TEXT, "Password"));
            let mut password = area.spawn((
                PasswordField,
                Field,
                Hovered::default(),
                field_node(),
                BackgroundColor(colors::FIELD),
                BorderColor::all(colors::FIELD_EDGE),
                TabIndex(first + 1),
                children![(
                    Dots,
                    Text::new(dots(form.password_len())),
                    f.regular(15.0),
                    TextColor(colors::TEXT),
                    Pickable::IGNORE,
                )],
            ));
            password.observe(on_password_key).observe(focus_on_press);
            if !form.handle.is_empty() {
                password.insert(AutoFocus);
            }
        }
        Step::Code { .. } => {
            let ask = if form.recovery {
                "One of your recovery codes"
            } else {
                "The code from your authenticator app"
            };
            area.spawn(line(f.regular(14.0), colors::TEXT, ask));
            area.spawn((
                CodeField,
                text_field(f, &form.code),
                TabIndex(first),
                AutoFocus,
            ));
        }
    });
}

/// The button that switches between the app's code and a recovery code, at the code's step.
pub fn code_choice(row: &mut ChildSpawnerCommands, f: &UiFonts, form: &Form) {
    if matches!(form.step, Step::Code { .. }) {
        let (label, recovery) = if form.recovery {
            ("Use the app's code", false)
        } else {
            ("Use a recovery code", true)
        };
        row.spawn(button(f, label, Action::UseRecoveryCode(recovery)));
    }
}

fn field_node() -> Node {
    Node {
        height: px(32),
        padding: UiRect::axes(px(8), px(5)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(6)),
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    }
}

pub(crate) fn text_field(f: &UiFonts, text: &str) -> impl Bundle {
    (
        Field,
        TextInput,
        EditableText::new(text),
        cursor_style(),
        Hovered::default(),
        f.regular(15.0),
        TextColor(colors::TEXT),
        field_node(),
        BackgroundColor(colors::FIELD),
        BorderColor::all(colors::FIELD_EDGE),
    )
}

/// The cursor Bevy draws in a field, the text's color (its default is a dark slate, unseen on
/// the dark fields), and what is selected.
fn cursor_style() -> TextCursorStyle {
    TextCursorStyle {
        color: colors::TEXT,
        selection_color: colors::SELECTION,
        unfocused_selection_color: Color::NONE,
        ..default()
    }
}

/// How long a cursor is shown, then hidden: half of Bevy's `cursor_blink_period`, so the
/// password's blinks as the other fields' do.
const BLINK: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether a cursor that started blinking `since` ago is shown now: at once, then every other
/// [`BLINK`].
fn caret_shown(since: std::time::Duration) -> bool {
    (since.as_millis() / BLINK.as_millis()).is_multiple_of(2)
}

fn dots(n: usize) -> String {
    "•".repeat(n)
}

/// Enter sends the form, from any of its fields.
fn on_form_key(
    mut key: On<FocusedInput<KeyboardInput>>,
    net: Res<Net>,
    mut signing: ResMut<Signing>,
) {
    if key.input.state != ButtonState::Pressed {
        return;
    }
    if key.input.logical_key == Key::Enter {
        key.propagate(false);
        submit(&net, &mut signing);
    }
}

/// A pasted password on its way from the clipboard.
#[derive(Resource, Default)]
struct Pasting(Option<ClipboardRead>);

/// The password's field takes typing, Backspace and pasting; Tab, Enter and Escape go on to the
/// form.
fn on_password_key(
    mut key: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut signing: ResMut<Signing>,
    mut clipboard: ResMut<Clipboard>,
    mut pasting: ResMut<Pasting>,
) {
    let input = key.input.clone();
    if input.state != ButtonState::Pressed {
        return;
    }
    if matches!(input.logical_key, Key::Tab | Key::Enter | Key::Escape) {
        return;
    }
    key.propagate(false);
    let Some(form) = signing.bypass_change_detection().form.as_mut() else {
        return;
    };
    let command = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    match &input.logical_key {
        Key::Backspace => form.erase_password(),
        Key::Character(c) if command && c.eq_ignore_ascii_case("v") => {
            pasting.0 = Some(clipboard.fetch_text());
        }
        _ if command || keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) => {}
        _ => {
            if let Some(text) = &input.text {
                form.type_password(text);
            }
        }
    }
}

fn focus_on_press(press: On<PointerPress>, mut focus: ResMut<InputFocus>) {
    focus.set(press.entity, FocusCause::Pressed);
}

fn paste_password(mut pasting: ResMut<Pasting>, mut signing: ResMut<Signing>) {
    let Some(read) = pasting.0.as_mut() else {
        return;
    };
    match read.poll_result() {
        Some(Ok(text)) => {
            if let Some(form) = signing.bypass_change_detection().form.as_mut() {
                form.type_password(&text);
            }
            pasting.0 = None;
        }
        Some(Err(e)) => {
            warn!("could not paste: {e}");
            pasting.0 = None;
        }
        None if matches!(read, ClipboardRead::Taken) => pasting.0 = None,
        None => {}
    }
}

/// What is typed in the handle's and the code's fields, kept in the form, so that a form drawn
/// again shows it.
/// The handle's and the code's fields, when what is typed in them changed.
type TypedFields<'w, 's> = Query<
    'w,
    's,
    (&'static EditableText, Has<CodeField>),
    (
        Changed<EditableText>,
        Or<(With<HandleField>, With<CodeField>)>,
    ),
>;

fn keep_typed(mut signing: ResMut<Signing>, fields: TypedFields) {
    for (text, code) in &fields {
        let Some(form) = signing.bypass_change_detection().form.as_mut() else {
            return;
        };
        let typed = text.value().to_string();
        let kept = if code {
            &mut form.code
        } else {
            &mut form.handle
        };
        if *kept != typed {
            *kept = typed;
        }
    }
}

/// When the password's cursor last started blinking: as the field took the keyboard, or as a
/// key changed what is in it, so that it is seen while typing.
#[derive(Default)]
struct Blink {
    since: std::time::Duration,
    focused: bool,
    len: usize,
}

fn show_dots(
    signing: Res<Signing>,
    focus: Res<InputFocus>,
    time: Res<Time<Real>>,
    mut blink: Local<Blink>,
    fields: Query<Entity, With<PasswordField>>,
    mut shown_in: Query<&mut Text, With<Dots>>,
) {
    let Some(form) = &signing.form else { return };
    let focused = fields.iter().any(|e| focus.get() == Some(e));
    let len = form.password_len();
    let now = time.elapsed();
    if focused != blink.focused || len != blink.len {
        *blink = Blink {
            since: now,
            focused,
            len,
        };
    }
    let caret = focused && caret_shown(now - blink.since);
    let shown = format!("{}{}", dots(len), if caret { "|" } else { "" });
    for mut text in &mut shown_in {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

/// A field's edge: lit while it has the keyboard, less while the pointer is over it. Only the
/// edge: the field itself stays as it is.
fn edge(focused: bool, hovered: bool) -> Color {
    if focused {
        colors::FIELD_FOCUS
    } else if hovered {
        colors::FIELD_HOVER
    } else {
        colors::FIELD_EDGE
    }
}

fn outline_fields(
    focus: Res<InputFocus>,
    mut fields: Query<(Entity, Option<&Hovered>, &mut BorderColor), With<Field>>,
) {
    for (entity, hovered, mut border) in &mut fields {
        let color = edge(
            focus.get() == Some(entity),
            hovered.is_some_and(|h| h.get()),
        );
        if border.top != color {
            *border = BorderColor::all(color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(handle: &str, password: &str) -> Form {
        let mut form = Form {
            handle: handle.into(),
            ..Default::default()
        };
        form.type_password(password);
        form
    }

    #[test]
    fn the_password_is_sent_once_and_then_forgotten() {
        let mut form = filled(" ana ", "hunter2");
        assert_eq!(
            form.request(),
            Some(Request::SignIn {
                handle: "ana".into(),
                password: "hunter2".into()
            })
        );
        assert!(form.busy);
        assert_eq!(form.password_len(), 0, "not kept once sent");
        assert_eq!(form.request(), None, "not sent twice while waiting");
        assert!(
            !format!("{form:?}").contains("hunter2"),
            "never in a log: {form:?}"
        );
    }

    #[test]
    fn nothing_is_sent_without_a_handle_and_a_password() {
        assert_eq!(filled("", "x").request(), None);
        assert_eq!(filled("ana", "").request(), None);
        assert_eq!(filled("  ", "x").request(), None);
    }

    #[test]
    fn typing_the_password_takes_printable_characters_and_backspace() {
        let mut form = Form::default();
        form.type_password("pa\tss\nwörd");
        assert_eq!(
            form.password_len(),
            8,
            "the tab and the newline are left out"
        );
        form.erase_password();
        form.erase_password();
        assert_eq!(form.password, "passwö");
    }

    /// Keys sent to the password's field, as a window sends them once the field has the keyboard.
    #[test]
    fn the_password_field_takes_keys_and_leaves_enter_to_the_form() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Clipboard>()
            .init_resource::<Pasting>()
            .insert_resource(Signing {
                form: Some(Form::default()),
                out_problem: None,
            });
        let form = app.world_mut().spawn_empty().observe(on_form_key).id();
        let field = app
            .world_mut()
            .spawn((PasswordField, ChildOf(form)))
            .observe(on_password_key)
            .id();
        // Bubbling up to the form reads whether an entity is a window, as a real app (with its
        // window plugin) can.
        app.world_mut().register_component::<Window>();
        let window = app.world_mut().spawn_empty().id();
        let mut press = |key: Key, code: KeyCode, text: Option<&str>| {
            let input = KeyboardInput {
                key_code: code,
                logical_key: key,
                state: ButtonState::Pressed,
                text: text.map(Into::into),
                repeat: false,
                window,
            };
            app.world_mut()
                .trigger(FocusedInput::new(field, input, window));
        };
        for c in ["s", "3", "c", "r"] {
            press(Key::Character(c.into()), KeyCode::KeyA, Some(c));
        }
        press(Key::Backspace, KeyCode::Backspace, None);
        press(Key::Space, KeyCode::Space, Some(" "));
        let signing = app.world().resource::<Signing>();
        assert_eq!(signing.form.as_ref().unwrap().password, "s3c ");

        // Enter is not typed: it goes on to the form, which asks for a handle first.
        app.world_mut()
            .resource_mut::<Signing>()
            .form
            .as_mut()
            .unwrap()
            .handle = "ana".into();
        let (net, requests) = Net::for_test();
        app.insert_resource(net);
        let input = KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: Some("\r".into()),
            repeat: false,
            window,
        };
        app.world_mut()
            .trigger(FocusedInput::new(field, input, window));
        assert_eq!(
            requests.try_recv(),
            Ok(Request::SignIn {
                handle: "ana".into(),
                password: "s3c ".into()
            })
        );
        let form = app.world().resource::<Signing>().form.clone().unwrap();
        assert_eq!(form.password_len(), 0);
        assert!(form.busy);
    }

    #[test]
    fn a_second_factor_asks_for_a_code_and_a_refusal_says_why() {
        let mut signing = Signing::default();
        signing.open("");
        let form = signing.form.as_mut().unwrap();
        form.handle = "ana".into();
        form.type_password("pw");
        assert!(form.request().is_some());

        signing.answered(Ok(SignIn::NeedsCode {
            pending: "p1".into(),
        }));
        let form = signing.form.as_mut().unwrap();
        assert_eq!(
            form.step,
            Step::Code {
                pending: "p1".into()
            }
        );
        assert!(!form.busy);
        assert_eq!(form.request(), None, "no code yet");
        form.code = " 123456 ".into();
        assert_eq!(
            form.request(),
            Some(Request::SecondFactor {
                pending: "p1".into(),
                code: Code::App("123456".into())
            })
        );

        let wrong = Problem::Refused {
            status: 401,
            message: "auth: that code is wrong".into(),
        };
        assert_eq!(signing.answered(Err(wrong.clone())), None);
        let form = signing.form.as_mut().unwrap();
        assert_eq!(form.problem, Some(wrong));
        assert!(form.code.is_empty() && !form.busy);
        form.recovery = true;
        form.code = "abcd-efgh".into();
        assert_eq!(
            form.request(),
            Some(Request::SecondFactor {
                pending: "p1".into(),
                code: Code::Recovery("abcd-efgh".into())
            })
        );

        let ana = Account {
            handle: "ana".into(),
            display_name: "Ana".into(),
            must_change_password: false,
        };
        assert_eq!(
            signing.answered(Ok(SignIn::Done(ana.clone()))),
            Some(SignIn::Done(ana))
        );
        assert_eq!(signing.form, None, "signed in, the form closes");
    }

    #[test]
    fn a_cursor_shows_at_once_then_blinks() {
        let at = |ms| caret_shown(std::time::Duration::from_millis(ms));
        assert!(
            at(0),
            "seen as soon as the field has the keyboard, or a key is typed"
        );
        assert!(at(499));
        assert!(!at(500));
        assert!(!at(999));
        assert!(at(1000));
    }

    #[test]
    fn the_cursor_is_seen_on_a_dark_field() {
        let style = cursor_style();
        assert_ne!(style.color, TextCursorStyle::default().color);
        assert_eq!(style.color, colors::TEXT);
    }

    #[test]
    fn a_fields_edge_is_lit_by_the_pointer_and_more_by_the_keyboard() {
        assert_eq!(edge(false, false), colors::FIELD_EDGE);
        assert_eq!(edge(false, true), colors::FIELD_HOVER);
        assert_eq!(edge(true, false), colors::FIELD_FOCUS);
        assert_eq!(edge(true, true), colors::FIELD_FOCUS);
    }

    /// Only the edge changes as the pointer comes over a field, then as it takes the keyboard.
    #[test]
    fn a_field_under_the_pointer_has_its_edge_lit_and_its_fill_left_alone() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<InputFocus>()
            .add_systems(Update, outline_fields);
        let field = app
            .world_mut()
            .spawn((
                Field,
                Hovered::default(),
                BackgroundColor(colors::FIELD),
                BorderColor::all(colors::FIELD_EDGE),
            ))
            .id();
        let looks = |app: &App| {
            let e = app.world().entity(field);
            (
                e.get::<BorderColor>().unwrap().top,
                e.get::<BackgroundColor>().unwrap().0,
            )
        };
        app.update();
        assert_eq!(looks(&app), (colors::FIELD_EDGE, colors::FIELD));

        app.world_mut().entity_mut(field).insert(Hovered(true));
        app.update();
        assert_eq!(looks(&app), (colors::FIELD_HOVER, colors::FIELD));

        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Pressed);
        app.update();
        assert_eq!(looks(&app), (colors::FIELD_FOCUS, colors::FIELD));

        app.world_mut().entity_mut(field).insert(Hovered(false));
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
        assert_eq!(looks(&app), (colors::FIELD_EDGE, colors::FIELD));
    }
}
