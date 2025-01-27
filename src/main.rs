use reqwest::Client;
use std::fs::{File, create_dir_all, read_dir, remove_file};
use std::io::{self, Write};
use tar::Archive;
use zstd::stream::read::Decoder;
use std::path::{Path, PathBuf};
use clap::{Command, Arg};
use dirs::home_dir;
use select::{document::Document, node::Node};
use select::predicate::Name;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::BufRead;

#[tokio::main]
async fn main() {
    // Set up the command-line argument parser with subcommands
    let matches = Command::new("hydropkg")
        .version("1.0")
        .author("hydrophobis")
        .about("hydrosh package manager")
        .subcommand(
            Command::new("install")
                .about("Install a package")
                .arg(
                    Arg::new("package")
                        .help("The name of the package to install")
                        .required(true)
                        .value_parser(clap::value_parser!(String)),
                ),
        )
        .subcommand(
            Command::new("search")
                .about("Search for packages")
                .arg(
                    Arg::new("search")
                        .help("Package to search for")
                        .required(true)
                        .value_parser(clap::value_parser!(String)),
                )
        )
        .subcommand(
            Command::new("remove")
                .about("Remove a package")
                .arg(
                    Arg::new("package")
                        .help("The name of the package to remove")
                        .required(true)
                        .value_parser(clap::value_parser!(String)),
                ),
        )
        .get_matches();

    // Handle the subcommands and their arguments
    if let Some(matches) = matches.subcommand_matches("install") {
        if let Some(package_name) = matches.get_one::<String>("package") {
            // Call the function to download and extract the package
            match download_and_extract_package(package_name).await {
                Ok(()) => {
                    println!("Package '{}' downloaded and extracted successfully!", package_name);
                    add_installed_package(package_name).unwrap();
                }
                Err(e) => eprintln!("Error downloading and extracting package: {}", e),
            }
        }
    } else if let Some(matches) = matches.subcommand_matches("search") {
        if let Some(search_query) = matches.get_one::<String>("search") {
            // Call the search function
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
        if let Some(package_name) = matches.get_one::<String>("package") {
            // Call the function to remove the package
            match remove_package(package_name).await {
                Ok(()) => println!("Package '{}' removed successfully!", package_name),
                Err(e) => eprintln!("Error removing package: {}", e),
            }
        }
    } else {
        println!("No valid subcommand provided. Use 'install <package>' to install a package or 'search <query>' to search for packages.");
    }
}

// Function to download and extract the package
async fn download_and_extract_package(package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Define the mirror base URL for Arch Linux
    let mirror = "https://mirror.rackspace.com/archlinux/core/os/x86_64/";
    let package_url = format!("{}{}.pkg.tar.zst", mirror, package_name);
    
    let client = Client::new();
    let response = client.get(&package_url).send().await?;
    
    // Check if the response is successful
    if !response.status().is_success() {
        println!("No package found, similar packages are");
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

    // Use zstd decoder for .pkg.tar.zst files
    let decoder = Decoder::new(&tarball[..])?;
    let mut archive = Archive::new(decoder);

    // Define the target directory where the package should be unpacked
    // Example: Use the root directory or any other custom directory
    let out_path = Path::new("/desired/unpack/path"); // Replace with desired unpack path
    
    // Ensure the directory exists before unpacking
    if !out_path.exists() {
        create_dir_all(out_path)?;
    }

    // Extract the package contents to the specified directory
    match archive.unpack(out_path) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to unpack archive: {}", e).into()),
    }
}


// Function to search for packages in the mirror directory
async fn search_package(search_query: &str) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    // Define the mirror base URL for Arch Linux
    let mirror = "https://mirror.rackspace.com/archlinux/core/os/x86_64/";

    let client = Client::new();
    let response = client.get(mirror).send().await?;

    // Check if the response is successful
    if !response.status().is_success() {
        return Err(format!("Failed to fetch package list: {}", response.status()).into());
    }

    let body = response.text().await?;

    // Parse the HTML to extract package links
    let document = Document::from(body.as_str());

    let mut result = HashSet::new();

    // Find all links to .pkg.tar.zst files and check if they match the search query
    for node in document.find(Name("a")) {
        if let Some(link) = node.attr("href") {
            if link.ends_with(".pkg.tar.zst") && link.contains(search_query) {
                // Insert package name without the .pkg.tar.zst extension
                let package_name = link.trim_end_matches(".pkg.tar.zst");
                result.insert(package_name.to_string());
            }
        }
    }

    Ok(result)
}

// Function to add the installed package to the list
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

// Function to remove a package and its binary
async fn remove_package(package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let home = home_dir().unwrap();
    let config_dir = home.join(".hydropkg");
    let installed_file = config_dir.join("installed.txt");

    // Read the installed packages
    let installed_packages: Vec<String> = std::fs::read_to_string(&installed_file)?
        .lines()
        .map(|line| line.to_string())
        .collect();

    // Check if the package is installed
    if !installed_packages.contains(&package_name.to_string()) {
        return Err(format!("Package '{}' is not installed", package_name).into());
    }

    // Remove the package from the installed list
    let updated_packages: Vec<String> = installed_packages
        .into_iter()
        .filter(|pkg| pkg != package_name)
        .collect();

    // Rewrite the installed file
    let mut file = File::create(installed_file)?;
    for package in updated_packages {
        writeln!(file, "{}", package)?;
    }

    // Delete the package binary (assuming it's located in the current directory)
    let package_binary = Path::new(package_name);
    if package_binary.exists() {
        remove_file(package_binary)?;
    }

    Ok(())
}
