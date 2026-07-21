#![forbid(unsafe_code)]

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{Name, Stream, prelude::*};
#[cfg(unix)]
use std::path::Path;
use std::{
    fs, io,
    time::{SystemTime, UNIX_EPOCH},
};
use supervisor_ipc::{Envelope, Request, Response, ResponseStatus, read_frame, write_frame};
use uuid::Uuid;

#[cfg(windows)]
const SOCKET_NAME: &str = "local-ai-image-filter.supervisor.v1";
#[cfg(unix)]
const UNIX_SOCKET_PATH: &str = "/var/run/local-ai-image-filter/supervisor.sock";

fn endpoint() -> io::Result<Name<'static>> {
    #[cfg(windows)]
    {
        SOCKET_NAME
            .to_ns_name::<GenericNamespaced>()
            .map_err(io::Error::other)
    }
    #[cfg(unix)]
    {
        Path::new(UNIX_SOCKET_PATH)
            .to_fs_name::<GenericFilePath>()
            .map(Name::into_owned)
            .map_err(io::Error::other)
    }
}

fn authorization(path: &str) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > 4096 {
        return Err(io::Error::other("authorization file is oversized"));
    }
    let value = fs::read_to_string(path)?;
    let value = value.trim();
    if value.is_empty() {
        return Err(io::Error::other("authorization file is empty"));
    }
    Ok(value.to_owned())
}

fn invoke(request: Request) -> io::Result<serde_json::Value> {
    let mut stream = Stream::connect(endpoint()?)?;
    let envelope = Envelope {
        protocol_version: 1,
        request_id: Uuid::new_v4(),
        nonce: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs() as i64,
        request,
    };
    write_frame(&mut stream, &envelope).map_err(io::Error::other)?;
    let response: Response<serde_json::Value> =
        read_frame(&mut stream).map_err(io::Error::other)?;
    if response.request_id != envelope.request_id {
        return Err(io::Error::other("supervisor response request ID mismatch"));
    }
    match (response.status, response.payload, response.error) {
        (ResponseStatus::Ok, Some(payload), None) => Ok(payload),
        (ResponseStatus::Error, None, Some(error)) => Err(io::Error::other(format!(
            "{}: {}",
            error.code, error.message
        ))),
        _ => Err(io::Error::other("malformed supervisor response")),
    }
}

fn request_from_arguments(arguments: &[String]) -> io::Result<Request> {
    match arguments {
        [command] if command == "status" => Ok(Request::GetStatus),
        [area, command] if area == "network" && command == "status" => Ok(Request::GetHealth),
        [area, command] if area == "certificate" && command == "status" => Ok(Request::GetHealth),
        [command, option, path]
            if command == "prepare-uninstall" && option == "--authorization-file" =>
        {
            Ok(Request::PrepareUninstall {
                authorization: authorization(path)?,
            })
        }
        [area, command, option, path]
            if area == "network" && command == "repair" && option == "--authorization-file" =>
        {
            Ok(Request::RepairNetworkConfiguration {
                authorization: authorization(path)?,
            })
        }
        [area, command, option, path]
            if area == "certificate"
                && matches!(command.as_str(), "install" | "remove")
                && option == "--authorization-file" =>
        {
            Ok(Request::InstallOrRepairCertificate {
                authorization: authorization(path)?,
            })
        }
        _ => Err(io::Error::other(
            "usage: filterctl status | network status | network repair --authorization-file PATH | certificate status | certificate install|remove --authorization-file PATH | prepare-uninstall --authorization-file PATH",
        )),
    }
}

fn main() -> io::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let response = invoke(request_from_arguments(&arguments)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).map_err(io::Error::other)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_commands_are_not_accepted() {
        assert!(request_from_arguments(&["exec".into(), "whoami".into()]).is_err());
    }
}
