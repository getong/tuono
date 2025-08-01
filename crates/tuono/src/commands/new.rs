use clap::crate_version;
use reqwest::blocking;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::env;
use std::fs::{self, File, OpenOptions, create_dir};
use std::io::{self, prelude::*};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::trace;

#[derive(Deserialize, Debug)]
enum GithubFileType {
    #[serde(rename = "blob")]
    Blob,
    #[serde(rename = "tree")]
    Tree,
}

#[derive(Deserialize, Debug)]
struct GithubTagObject {
    sha: String,
}

#[derive(Deserialize, Debug)]
struct GithubTagResponse {
    object: GithubTagObject,
}

#[derive(Deserialize, Debug)]
struct GithubTreeResponse<T> {
    tree: Vec<T>,
}

fn exit_with_error(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

#[derive(Deserialize, Debug)]
struct GithubFile {
    path: String,
    #[serde(rename(deserialize = "type"))]
    element_type: GithubFileType,
}

fn create_file(path: PathBuf, content: String) -> std::io::Result<()> {
    let mut file = File::create(&path).unwrap_or_else(|err| {
        exit_with_error(&format!(
            "Failed to create file {}: {}",
            path.display(),
            err
        ));
    });
    let _ = file.write_all(content.as_bytes());

    Ok(())
}

pub fn create_new_project(
    folder_name: Option<String>,
    template: Option<String>,
    select_head: Option<bool>,
) {
    let folder = folder_name.unwrap_or(".".to_string());

    let github_api_base_url =
        env::var("__INTERNAL_TUONO_TEST").unwrap_or("https://api.github.com".to_string());

    let github_raw_base_url = env::var("__INTERNAL_TUONO_TEST")
        .unwrap_or("https://raw.githubusercontent.com".to_string());

    // In case of missing select the tuono example
    let template = template.unwrap_or("tuono-app".to_string());
    let client = blocking::Client::builder()
        .user_agent("")
        .build()
        .unwrap_or_else(|_| exit_with_error("Error: Failed to build request client"));

    // This string does not include the "v" version prefix
    let cli_version: &str = crate_version!();

    let tree_url: String =
        generate_tree_url(select_head, &client, cli_version, &github_api_base_url);

    let res_tree = client
        .get(tree_url)
        .send()
        .unwrap_or_else(|_| {
            exit_with_error(&format!(
                "Failed to call the tagged commit tree github API for v{cli_version}"
            ))
        })
        .json::<GithubTreeResponse<GithubFile>>()
        .expect("Failed to parse the tree structure");

    let new_project_files = res_tree
        .tree
        .iter()
        .filter(|GithubFile { path, .. }| path.starts_with(&format!("examples/{template}/")))
        .collect::<Vec<&GithubFile>>();

    if new_project_files.is_empty() {
        eprintln!("Error: Template '{template}' not found");
        println!(
            "Hint: you can view the available templates at https://github.com/tuono-labs/tuono/tree/main/examples"
        );
        std::process::exit(1);
    }

    if folder != "." {
        if Path::new(&folder).exists() {
            eprintln!("Error: Directory '{folder}' already exists");
            println!(
                "Hint: you can scaffold a tuono project within an existing folder with 'cd {folder} && tuono new .'"
            );
            std::process::exit(1);
        }
        create_dir(&folder).unwrap();
    }

    let folder_name = PathBuf::from(&folder);
    let current_dir = env::current_dir().expect("Failed to get current working directory");

    let folder_path = current_dir.join(folder_name);

    create_directories(&new_project_files, &folder_path, &template)
        .unwrap_or_else(|err| exit_with_error(&format!("Failed to create directories: {err}")));

    for GithubFile {
        element_type, path, ..
    } in new_project_files.iter()
    {
        if let GithubFileType::Blob = element_type {
            let url =
                generate_raw_content_url(select_head, cli_version, path, &github_raw_base_url);

            let file_content = client
                .get(url)
                .send()
                .map_err(|_| exit_with_error("Failed to call the folder github API"))
                .and_then(|response| {
                    response
                        .text()
                        .map_err(|_| exit_with_error("Failed to parse the repo structure"))
                })
                .unwrap();

            let path = PathBuf::from(&path.replace(&format!("examples/{template}/"), ""));

            let file_path = folder_path.join(&path);

            if let Err(err) = create_file(file_path, file_content) {
                exit_with_error(&format!("Failed to create file: {err}"));
            }
        }
    }

    update_package_json_version(&folder_path).expect("Failed to update package.json version");
    update_cargo_toml_version(&folder_path).expect("Failed to update Cargo.toml version");

    init_new_git_repo(&folder_path);

    outro(folder);
}

fn generate_raw_content_url(
    select_head: Option<bool>,
    cli_version: &str,
    path: &String,
    url: &str,
) -> String {
    let tag = if select_head.unwrap_or(false) {
        "/main"
    } else {
        &format!("/tuono-labs/tuono/v{cli_version}")
    };
    format!("{url}{tag}/{path}")
}

fn generate_tree_url(
    select_head: Option<bool>,
    client: &Client,
    cli_version: &str,
    url: &str,
) -> String {
    if select_head.unwrap_or(false) {
        format!("{url}/repos/tuono-labs/tuono/git/trees/main?recursive=1")
    } else {
        // This string does not include the "v" version prefix
        let res_tag = client
            .get(format!(
                "{url}/repos/tuono-labs/tuono/git/ref/tags/v{cli_version}"
            ))
            .send()
            .unwrap_or_else(|_| {
                exit_with_error("Failed to call the tag github API for v{cli_version}")
            })
            .json::<GithubTagResponse>()
            .unwrap_or_else(|_| exit_with_error("Failed to parse the tag response"));

        format!(
            "{url}/repos/tuono-labs/tuono/git/trees/{}?recursive=1",
            res_tag.object.sha
        )
    }
}

fn create_directories(
    new_project_files: &[&GithubFile],
    folder_path: &Path,
    template: &String,
) -> io::Result<()> {
    for GithubFile {
        element_type, path, ..
    } in new_project_files.iter()
    {
        if let GithubFileType::Tree = element_type {
            let path = PathBuf::from(&path.replace(&format!("examples/{template}/"), ""));

            let dir_path = folder_path.join(&path);
            if let Err(e) = create_dir(&dir_path) {
                eprintln!("Failed to create directory {}: {}", dir_path.display(), e);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
fn update_package_json_version(folder_path: &Path) -> io::Result<()> {
    let v = crate_version!();
    let package_json_path = folder_path.join(PathBuf::from("package.json"));
    let package_json = fs::read_to_string(&package_json_path)
        .unwrap_or_else(|err| exit_with_error(&format!("Failed to read package.json: {err}")));
    let package_json = package_json.replace("link:../../packages/tuono", v);

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(package_json_path)
        .unwrap_or_else(|err| exit_with_error(&format!("Failed to open package.json: {err}")));

    file.write_all(package_json.as_bytes())
        .unwrap_or_else(|err| exit_with_error(&format!("Failed to write to package.json: {err}")));

    Ok(())
}

fn update_cargo_toml_version(folder_path: &Path) -> io::Result<()> {
    let v = crate_version!();
    let cargo_toml_path = folder_path.join(PathBuf::from("Cargo.toml"));
    let cargo_toml = fs::read_to_string(&cargo_toml_path)
        .unwrap_or_else(|err| exit_with_error(&format!("Failed to read Cargo.toml: {err}")));
    let cargo_toml = cargo_toml.replace(
        "{ path = \"../../crates/tuono_lib/\" }",
        &format!("\"{v}\""),
    );

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(cargo_toml_path)
        .unwrap_or_else(|err| exit_with_error(&format!("Failed to open Cargo.toml: {err}")));

    file.write_all(cargo_toml.as_bytes())
        .unwrap_or_else(|err| exit_with_error(&format!("Failed to write to Cargo.toml: {err}")));

    Ok(())
}

fn init_new_git_repo(folder_path: &Path) {
    if let Ok(output) = Command::new("git").arg("init").arg(folder_path).output() {
        if !output.status.success() {
            trace!("Failed to initialise a new git repo")
        }
    } else {
        trace!("Failed to initialise a new git repo")
    }
}

fn outro(folder_name: String) {
    println!("Success! 🎉");

    if folder_name != "." {
        println!("\nGo to the project directory:");
        println!("cd {folder_name}/");
    }

    println!("\nInstall the dependencies:");
    println!("npm install");

    println!("\nRun the local environment:");
    println!("tuono dev");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generate_valid_content_url_from_head() {
        let expected = format!(
            "{}/{}/{}",
            "http://localhost:3000", "main", "examples/tuono-app"
        );
        let generated = generate_raw_content_url(
            Some(true),
            crate_version!(),
            &String::from("examples/tuono-app"),
            "http://localhost:3000",
        );
        assert_eq!(expected, generated)
    }

    #[test]
    fn generate_valid_content_url_from_cli_version() {
        let expected = format!(
            "{}/{}/{}",
            "http://localhost:3000",
            &format!("tuono-labs/tuono/v{}", crate_version!()),
            "examples/tuono-app"
        );
        let generated = generate_raw_content_url(
            Some(false),
            crate_version!(),
            &String::from("examples/tuono-app"),
            "http://localhost:3000",
        );
        assert_eq!(expected, generated)
    }
}
