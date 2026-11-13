use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use eframe::egui;
use gitrust_desktop::{
    app::GitRustApp,
    git::{GitBackend, LocalGit},
    storage,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let startup = Instant::now();
    let mut initial_path = None;
    let mut persist = true;
    let mut smoke = false;
    let mut inspect = false;
    for argument in std::env::args_os().skip(1) {
        match argument.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "GitRust Desktop\nUsage: gitrust-desktop [--no-persist] [--smoke-test | --inspect] [repository-path]\n\n--smoke-test  Open the native window and exit after 3 seconds (no Git writes).\n--inspect     Print repository status/timing without a window (no Git writes).\n--no-persist  Do not load or save preferences."
                );
                return Ok(());
            }
            Some("--no-persist") => persist = false,
            Some("--smoke-test") => smoke = true,
            Some("--inspect") => inspect = true,
            Some(value) if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}").into());
            }
            _ if initial_path.is_none() => initial_path = Some(PathBuf::from(argument)),
            _ => return Err("Only one repository path is accepted.".into()),
        }
    }
    if inspect {
        let path = initial_path.unwrap_or(std::env::current_dir()?);
        let snapshot = LocalGit.snapshot(&path, &AtomicBool::new(false))?;
        println!(
            "Repository: {}\nHEAD: {}",
            snapshot.root.display(),
            snapshot.head
        );
        for file in snapshot.files {
            println!("{:?}\t{}\t{}", file.side, file.label, file.path.display());
        }
        for blocker in snapshot.write_blockers {
            println!("Read-only: {blocker}");
        }
        eprintln!(
            "metric inspect_ready_ms={:.2}",
            startup.elapsed().as_secs_f64() * 1000.0
        );
        return Ok(());
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 520.0])
            .with_app_id("dev.gitrust.Desktop"),
        ..Default::default()
    };
    #[cfg(feature = "glow")]
    {
        options.renderer = eframe::Renderer::Glow;
    }
    #[cfg(all(feature = "wgpu", not(feature = "glow")))]
    {
        options.renderer = eframe::Renderer::Wgpu;
    }
    let preferences = if persist {
        storage::default_path()
    } else {
        None
    };
    eframe::run_native(
        "GitRust Desktop",
        options,
        Box::new(move |cc| {
            Ok(Box::new(GitRustApp::new(
                &cc.egui_ctx,
                initial_path,
                preferences,
                startup,
                smoke,
            )?))
        }),
    )?;
    Ok(())
}
