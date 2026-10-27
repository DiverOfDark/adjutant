//! The quick-capture shortcut on Wayland, through the desktop's
//! GlobalShortcuts portal (KDE Plasma 6, GNOME 48+). Wayland doesn't let
//! apps grab keys themselves; the desktop asks the user to confirm the
//! binding once and owns it from then on (System Settings → Shortcuts).

use std::sync::OnceLock;

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut};
use futures_util::StreamExt;
use tauri::AppHandle;
use tokio::sync::mpsc;

const SHORTCUT_ID: &str = "quick-capture";
const APP_ID: &str = "dev.adjutant.app";

/// Asks the running portal session to open the desktop's shortcut editor.
static CONFIGURE: OnceLock<mpsc::UnboundedSender<()>> = OnceLock::new();

/// Starts the portal session once; later calls are no-ops.
pub fn start(app: &AppHandle, preferred: &str) {
    if CONFIGURE.get().is_some() {
        return;
    }
    let (tx, rx) = mpsc::unbounded_channel();
    if CONFIGURE.set(tx).is_err() {
        return;
    }
    let app = app.clone();
    let trigger = xdg_trigger(preferred);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(&app, trigger, rx).await {
            tracing::warn!("global shortcuts portal: {e}");
            crate::desktop::set_hotkey(
                &app,
                None,
                Some(format!(
                    "Your desktop didn't accept a global shortcut ({e}). Add a system shortcut that runs {}",
                    crate::desktop::capture_command()
                )),
            );
        }
    });
}

/// Opens the desktop's own dialog for changing the shortcut (portal v2).
pub fn configure() -> Result<(), String> {
    CONFIGURE
        .get()
        .ok_or("the desktop shortcut isn't set up")?
        .send(())
        .map_err(|_| "the desktop shortcut session ended".to_string())
}

fn trigger_of(shortcuts: &[Shortcut]) -> Option<String> {
    shortcuts
        .iter()
        .find(|s| s.id() == SHORTCUT_ID)
        .map(|s| s.trigger_description().trim().to_string())
        .filter(|t| !t.is_empty())
}

async fn run(
    app: &AppHandle,
    trigger: Option<String>,
    mut configure: mpsc::UnboundedReceiver<()>,
) -> ashpd::Result<()> {
    if let Err(e) = ensure_desktop_entry() {
        tracing::warn!("desktop entry: {e}");
    }
    // Tells the portal which app this is; older portals lack the registry.
    if let Ok(id) = APP_ID.try_into()
        && let Err(e) = ashpd::register_host_app(id).await
    {
        tracing::debug!("portal registry: {e}");
    }
    let proxy = GlobalShortcuts::new().await?;
    let session = proxy.create_session(Default::default()).await?;
    let shortcut = NewShortcut::new(SHORTCUT_ID, "Adjutant: quick capture")
        .preferred_trigger(trigger.as_deref());
    let bound = proxy
        .bind_shortcuts(&session, &[shortcut], None, Default::default())
        .await?
        .response()?;
    crate::desktop::set_hotkey(app, trigger_of(bound.shortcuts()), None);

    let mut activated = proxy.receive_activated().await?;
    let mut changed = proxy.receive_shortcuts_changed().await?;
    loop {
        tokio::select! {
            Some(a) = activated.next() => {
                if a.shortcut_id() == SHORTCUT_ID {
                    crate::desktop::toggle_capture(app);
                }
            }
            Some(c) = changed.next() => {
                crate::desktop::set_hotkey(app, trigger_of(c.shortcuts()), None);
            }
            Some(()) = configure.recv() => {
                if let Err(e) = proxy.configure_shortcuts(&session, None, Default::default()).await {
                    tracing::warn!("configure shortcuts: {e}");
                }
            }
            else => break,
        }
    }
    Ok(())
}

const GENERATED: &str = "X-Adjutant-Generated=true";

/// The portal only serves host apps it can match to a desktop entry named
/// after the app ID. Packages install one; an AppImage or a dev build gets
/// a hidden one in ~/.local/share/applications, kept pointing at this binary.
fn ensure_desktop_entry() -> std::io::Result<()> {
    use std::path::PathBuf;
    let file = format!("applications/{APP_ID}.desktop");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")))
        .ok_or_else(|| std::io::Error::other("no home directory"))?;
    let system_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    if system_dirs
        .split(':')
        .any(|d| PathBuf::from(d).join(&file).is_file())
    {
        return Ok(());
    }
    let path = data_home.join(&file);
    let existing = std::fs::read_to_string(&path).ok();
    if existing.as_deref().is_some_and(|c| !c.contains(GENERATED)) {
        return Ok(()); // the user's own entry
    }
    let exe = std::env::var("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|_| std::env::current_exe())?;
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=Adjutant\nComment=Quick capture shortcut for Adjutant\nExec=\"{}\" %U\nIcon={APP_ID}\nNoDisplay=true\n{GENERATED}\n",
        exe.display()
    );
    if existing.as_deref() != Some(content.as_str()) {
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(&path, content)?;
    }
    let icon = data_home.join(format!("icons/hicolor/128x128/apps/{APP_ID}.png"));
    if !icon.exists() {
        std::fs::create_dir_all(icon.parent().expect("has parent"))?;
        std::fs::write(&icon, include_bytes!("../icons/128x128.png"))?;
    }
    Ok(())
}

/// "CommandOrControl+Shift+Space" -> "CTRL+SHIFT+space", the trigger format of
/// the XDG shortcuts spec. `None` for an empty hotkey.
fn xdg_trigger(hotkey: &str) -> Option<String> {
    let parts: Vec<String> = hotkey
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "commandorcontrol" | "cmdorctrl" | "control" | "ctrl" => "CTRL".into(),
            "shift" => "SHIFT".into(),
            "alt" | "option" => "ALT".into(),
            "super" | "meta" | "command" | "cmd" => "LOGO".into(),
            "space" => "space".into(),
            "enter" | "return" => "Return".into(),
            "esc" | "escape" => "Escape".into(),
            _ if p.chars().count() == 1 => p.to_lowercase(),
            _ => p.to_string(),
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::xdg_trigger;

    #[test]
    fn converts_tauri_accelerators() {
        assert_eq!(
            xdg_trigger("CommandOrControl+Shift+Space").as_deref(),
            Some("CTRL+SHIFT+space")
        );
        assert_eq!(xdg_trigger("Alt+N").as_deref(), Some("ALT+n"));
        assert_eq!(xdg_trigger("Super+F2").as_deref(), Some("LOGO+F2"));
        assert_eq!(xdg_trigger(" "), None);
    }
}
