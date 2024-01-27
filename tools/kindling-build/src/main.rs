// Copyright (c) 2023 the Hearth contributors.
// Copyright (c) 2023 Marceline Cramer
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This file is part of Hearth.
//
// Hearth is free software: you can redistribute it and/or modify it under the
// terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// Hearth is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
// details.
//
// You should have received a copy of the GNU Affero General Public License
// along with Hearth. If not, see <https://www.gnu.org/licenses/>.

use std::{
    path::Path,
    process::{Command, Stdio},
};

use cargo_metadata::Package;

fn main() {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(get_kindling_dir())
        .exec()
        .expect("failed to get cargo metadata");

    let target_path = metadata.target_directory.as_std_path().to_owned();
    let root_path = target_path.join("kindling-root");

    let is_clean = touch_dir(&root_path);

    build_wasm("kindling-init", &root_path.join("init.wasm"), is_clean);

    for package_id in metadata.workspace_members.iter() {
        let package = &metadata[package_id];

        let mut is_lib = false;
        for target in package.targets.iter() {
            if target.kind.contains(&"cdylib".to_string()) {
                is_lib = true;
                break;
            }
        }

        if !is_lib {
            eprintln!("{:?} is not a lib; skipping", package.name);
            continue;
        }

        build_service(&root_path, package);
    }
}

/// Returns true if the directory is freshly created.
fn touch_dir(path: &Path) -> bool {
    eprintln!("touching directory {:?}", path);
    // TODO ignore already exists error, catch everything else
    let _ = std::fs::create_dir_all(path);
    true
}

fn get_cargo() -> String {
    std::env::var("CARGO").expect("CARGO env var isn't set")
}

fn get_kindling_dir() -> String {
    std::env::var("CARGO_WORKSPACE_DIR").expect("CARGO_WORKSPACE_DIR env var isn't set")
        + "kindling/"
}

fn build_service(root_path: &Path, package: &Package) {
    if let Some(service) = package.metadata.get("service") {
        let service = service.as_object().unwrap();
        let name = service.get("name").unwrap().as_str().unwrap();
        let service_path = root_path.join("init").join(name);
        let is_clean = touch_dir(&service_path);
        let module_path = service_path.join("service.wasm");
        build_wasm(&package.name, &module_path, is_clean);

        // build a config file for the service
        let mut config = serde_json::Map::new();

        // write the description field with the Cargo package description
        if let Some(description) = package.description.clone() {
            config.insert("description".into(), description.into());
        }

        // copy over all fields from metadata
        // shadows build system-provided fields if specified in metadata
        for (k, v) in service.clone() {
            config.insert(k, v);
        }

        let config = toml::to_string_pretty(&config).unwrap();
        let config_path = service_path.join("service.toml");
        std::fs::write(config_path, config.as_bytes()).unwrap();
    }
}

fn build_wasm(package: &str, path: &Path, force_copy: bool) {
    let mut command = Command::new(get_cargo());
    command
        .current_dir(get_kindling_dir())
        .stdout(Stdio::piped())
        .arg("build")
        .arg("--message-format=json-render-diagnostics")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .arg("--package")
        .arg(package);

    eprintln!("executing command: {:?}", command);

    let mut child = command.spawn().expect("failed to run cargo command");

    let reader = std::io::BufReader::new(child.stdout.take().unwrap());
    for message in cargo_metadata::Message::parse_stream(reader) {
        use cargo_metadata::Message;
        if let Message::CompilerArtifact(artifact) = message.unwrap() {
            for file in artifact.filenames {
                if file.as_str().ends_with(".wasm") {
                    if artifact.fresh && !force_copy {
                        continue;
                    }

                    eprintln!("copying {:?} to {:?}", file, path);
                    std::fs::copy(file, path).unwrap();
                }
            }
        }
    }

    child.wait().unwrap();
}
