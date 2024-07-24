use std::env;
use std::fs::Permissions;
use std::process::exit;
use std::path::Path;
use chrono::Local;
use tokio::fs;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use async_compression::tokio::write::GzipEncoder;
use tokio_tar::{Builder, Header};

const DEFAULT_SEGMENT_SIZE: u64 = 4 * 1024 * 1024 * 1024; // 4GB in bytes

async fn create_backup() -> Result<(), Box<dyn std::error::Error>> {
    // Get the current working directory
    let cwd = env::current_dir()?;
    let cwd_str = cwd.to_str().unwrap();
    println!("Current working directory: {}", cwd_str);

    // Get the home directory
    let home_dir = env::var("HOME")?;
    println!("Home directory: {}", home_dir);

    // Replace "/" with "#" in the current working directory and create the filename
    let formatted_cwd = cwd_str.replacen(&home_dir, "", 1).replace("/", "#");
    let date = Local::now().format("%Y-%m-%d").to_string();
    let base_filename = format!("{}{}-{}", home_dir, formatted_cwd, date);
    println!("Backup base filename: {}", base_filename);

    // Check for the BACKUP_SEGMENT_SIZE environment variable
    let segment_size = match env::var("BACKUP_SEGMENT_SIZE") {
        Ok(size_str) => size_str.parse::<u64>().unwrap_or(DEFAULT_SEGMENT_SIZE),
        Err(_) => DEFAULT_SEGMENT_SIZE,
    };
    println!("Using segment size: {} bytes", segment_size);

    if !inquire::Confirm::new("Please check the info above, proceed?").prompt().unwrap() {
        println!("Backup creation aborted.");
        exit(0);
    }

    let mut segment_count = 0;
    let mut current_size = 0;
    let mut tar = new_tar_segment(&base_filename, segment_count).await?;

    // Add the contents of the current directory to the tar file
    let mut entries = fs::read_dir(&cwd).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = entry.metadata().await?;
        let mut header = Header::new_gnu();
        header.set_metadata(&metadata);
        header.set_path(path.strip_prefix(&cwd)?)?;
        println!("Adding {:?} to the backup.", path);

        if metadata.is_file() {
            let mut file = fs::File::open(&path).await?;
            let mut buf = vec![0; 1024 * 1024]; // 1MB buffer
            loop {
                let n = file.read(&mut buf).await?;
                if n == 0 { break; }
                tar.append_data(&mut header, path.file_name().unwrap_or_default(), &buf[..n]).await?;
                current_size += n as u64;
                if current_size >= segment_size {
                    tar.finish().await?;
                    segment_count += 1;
                    tar = new_tar_segment(&base_filename, segment_count).await?;
                    current_size = 0;
                }
            }
        } else if metadata.is_dir() {
            tar.append_dir_all(path.file_name().unwrap_or_default(), &path).await?;
        }
    }

    // Finish the last tar archive
    tar.finish().await?;
    println!("Backup file created.");
    Ok(())
}

async fn new_tar_segment(base_filename: &str, segment_count: u64) -> Result<Builder<GzipEncoder<fs::File>>, Box<dyn std::error::Error>> {
    let filename = format!("{}.part{}", base_filename, segment_count);
    let file = fs::File::create(&filename).await?;
    let enc = GzipEncoder::new(file);
    Ok(Builder::new(enc))
}

#[tokio::main]
async fn main() {
    match create_backup().await {
        Ok(_) => println!("Backup created successfully."),
        Err(e) => eprintln!("Error creating backup: {}", e),
    }
}
