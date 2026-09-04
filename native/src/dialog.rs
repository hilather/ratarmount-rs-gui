pub fn pick_file() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Open archive")
        .add_filter(
            "Archives",
            &["tar", "gz", "zst", "zip", "7z", "tgz", "tbz", "bz2", "xz"],
        )
        .pick_file()
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn pick_dir() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose folder")
        .pick_folder()
        .map(|p| p.to_string_lossy().into_owned())
}
