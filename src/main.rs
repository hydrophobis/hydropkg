use clap::builder::{IntoResettable, ValueRange};
use reqwest::Client;
use std::fs::{File, create_dir_all, remove_file};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use clap::{Command, Arg};
use dirs::home_dir;
use select::{document::Document, node::Node};
use select::predicate::Name;
use std::collections::HashSet;
use std::fs::OpenOptions;

#[tokio::main]
async fn main() {
    // Set up the command-line argument stuff
    let matches = Command::new("hydropkg")
        .version("1.0")
        .author("hydrophobis")
        .about("hydrosh package manager")
        .subcommand(
            Command::new("install")
                .about("Install packages")
                .arg(
                    Arg::new("packages")
                        .help("List of packages to install")
                        .required(true)
                        .value_parser(clap::value_parser!(String))
                        .num_args(ValueRange::new(1))
                ),
        )
        .subcommand(
            Command::new("search")
                .about("Search for packages")
                .arg(
                    Arg::new("search")
                        .help("Package to search for")
                        .required(true)
                        .value_parser(clap::value_parser!(String))
                        .num_args(ValueRange::new(1))
                )
        )
        .subcommand(
            Command::new("remove")
                .about("Remove packages")
                .arg(
                    Arg::new("packages")
                        .help("List of packages to remove")
                        .required(true)
                        .value_parser(clap::value_parser!(String))
                        .num_args(ValueRange::new(1))
                ),
        )
        .get_matches();

    // Handle the subcommands and their args
    if let Some(matches) = matches.subcommand_matches("install") {
        if let Some(package_names) = matches.get_many::<String>("packages") {
            // Download and extract each package
            for package_name in package_names {
                match download_and_extract_package(package_name).await {
                    Ok(()) => {
                        println!("Package '{}' downloaded and extracted successfully!", package_name);
                        if let Err(e) = add_installed_package(package_name) {
                            eprintln!("Error adding package to the installed list: {}", e);
                        }
                    }
                    Err(e) => eprintln!("Error downloading and extracting package '{}': {}", package_name, e),
                }
            }
        }
    } else if let Some(matches) = matches.subcommand_matches("search") {
        if let Some(search_query) = matches.get_one::<String>("search") {
            match search_package(search_query).await {
                Ok(packages) => {
                    if packages.is_empty() {
                        println!("No packages found for '{}'", search_query);
                    } else {
                        println!("Found the following packages:");
                        for package in packages {
                            println!("{}", package);
                        }
                    }
                }
                Err(e) => eprintln!("Error searching for package: {}", e),
            }
        }
    } else if let Some(matches) = matches.subcommand_matches("remove") {
        if let Some(package_names) = matches.get_many::<String>("packages") {
            for package_name in package_names {
                match remove_package(package_name).await {
                    Ok(()) => println!("Package '{}' removed successfully!", package_name),
                    Err(e) => eprintln!("Error removing package '{}': {}", package_name, e),
                }
            }
        }
    } else {
        println!("No valid subcommand provided. Use 'install <package>' to install a package or 'search <query>' to search for packages.");
    }
}

async fn download_and_extract_package(package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mirror = "https://mirror.rackspace.com/archlinux/core/os/x86_64/";
    let package_url = format!("{}{}.pkg.tar.zst", mirror, package_name);
    
    let client = Client::new();
    let response = client.get(&package_url).send().await?;
    
    if !response.status().is_success() {
        println!("No package found, similar packages are:");
        match search_package(package_name).await {
            Ok(packages) => {
                if packages.is_empty() {
                    println!("No packages found for '{}'", package_name);
                } else {
                    println!("Found the following packages:");
                    for package in packages {
                        println!("{}", package);
                    }
                }
            }
            Err(e) => eprintln!("Error searching for package: {}", e),
        }
        return Err(format!("Failed to download package: {}", response.status()).into());
    }

    let tarball = response.bytes().await?;
    let decoder = zstd::stream::read::Decoder::new(&tarball[..])?;
    let mut archive = tar::Archive::new(decoder);
    let out_path = Path::new("/bin/");

    if !out_path.exists() {
        create_dir_all(out_path)?;
    }

    match archive.unpack(out_path) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to unpack archive: {}", e).into()),
    }
}

async fn search_package(search_query: &str) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let mirror = "https://mirror.rackspace.com/archlinux/core/os/x86_64/";
    let client = Client::new();
    let response = client.get(mirror).send().await?;
    
    if !response.status().is_success() {
        return Err(format!("Failed to fetch package list: {}", response.status()).into());
    }

    let body = response.text().await?;
    let document = Document::from(body.as_str());

    let mut result = HashSet::new();

    for node in document.find(Name("a")) {
        if let Some(link) = node.attr("href") {
            if link.ends_with(".pkg.tar.zst") && link.contains(search_query) {
                let package_name = link.trim_end_matches(".pkg.tar.zst");
                result.insert(package_name.to_string());
            }
        }
    }

    Ok(result)
}

fn add_installed_package(package_name: &str) -> io::Result<()> {
    let home = home_dir().unwrap();
    let config_dir = home.join(".hydropkg");
    create_dir_all(&config_dir)?;

    let installed_file = config_dir.join("installed.txt");

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(installed_file)?;

    writeln!(file, "{}", package_name)?;
    Ok(())
}

async fn remove_package(package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let home = home_dir().unwrap();
    let config_dir = home.join(".hydropkg");
    let installed_file = config_dir.join("installed.txt");

    let installed_packages: Vec<String> = std::fs::read_to_string(&installed_file)?
        .lines()
        .map(|line| line.to_string()) //s
        .collect();

    if !installed_packages.contains(&package_name.to_string()) {
        return Err(format!("Package '{}' is not installed", package_name).into());
    }

    let updated_packages: Vec<String> = installed_packages
        .into_iter()
        .filter(|pkg| pkg != package_name)
        .collect();

    let mut file = File::create(installed_file)?;
    for package in updated_packages {
        writeln!(file, "{}", package)?;
    }

    let package_binary = Path::new("/bin/").join(package_name);
    if package_binary.exists() {
        remove_file(package_binary)?;
    }

    Ok(())
}
