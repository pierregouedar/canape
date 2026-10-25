//! Windows service plumbing: dispatcher entry point, install and uninstall.

use std::ffi::OsString;
use std::sync::mpsc;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

const NAME: &str = "CanapeService";

define_windows_service!(ffi_service_main, service_main);

/// Hands the thread to the service manager; fails when not started by it.
pub fn run_dispatcher() -> windows_service::Result<()> {
    service_dispatcher::start(NAME, ffi_service_main)
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(err) = run() {
        crate::log(&format!("service failed: {err}"));
    }
}

fn status(state: ServiceState, controls: ServiceControlAccept) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: controls,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}

fn run() -> windows_service::Result<()> {
    let (stop_tx, stop_rx) = mpsc::channel();
    let handle = service_control_handler::register(NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = stop_tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;

    let device = canape_hw::detect();
    crate::log(&format!("service starting, device = {}", device.name()));
    for line in device.diagnostics() {
        crate::log(&line);
    }
    if let Err(err) = crate::server::spawn(
        device,
        crate::tweaks::system(),
        crate::state_path(),
        crate::log,
    ) {
        crate::log(&format!("cannot serve the pipe: {err}"));
        let mut stopped = status(ServiceState::Stopped, ServiceControlAccept::empty());
        stopped.exit_code = ServiceExitCode::Win32(err.raw_os_error().unwrap_or(1) as u32);
        return handle.set_service_status(stopped);
    }
    handle.set_service_status(status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    ))?;

    let _ = stop_rx.recv();
    crate::log("service stopping");
    handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()))
}

/// Registers the service (automatic start, LocalSystem, restart on failure)
/// and starts it. The executable must already sit at its final location.
pub fn install() -> windows_service::Result<()> {
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )?;
    let executable_path = std::env::current_exe().map_err(windows_service::Error::Winapi)?;
    let info = ServiceInfo {
        name: NAME.into(),
        display_name: "Canapé".into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let service =
        manager.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
    service.set_description("Matériel, TDP et auto TDP de la ROG Ally")?;
    let restart = ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(2),
    };
    service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 3600)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart.clone(), restart.clone(), restart]),
    })?;
    service.start::<&str>(&[])?;
    println!("{NAME} installed and started");
    Ok(())
}

/// Stops the service if it runs, then deletes it.
pub fn uninstall() -> windows_service::Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(
        NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    if service.query_status()?.current_state != ServiceState::Stopped {
        service.stop()?;
        for _ in 0..50 {
            if service.query_status()?.current_state == ServiceState::Stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    service.delete()?;
    println!("{NAME} removed");
    Ok(())
}
