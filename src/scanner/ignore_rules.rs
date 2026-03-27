use std::path::Path;

use ignore::WalkBuilder;

pub fn build_walk(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .add_custom_ignore_filename(".duskignore")
        .follow_links(false)
        .git_ignore(true)
        .hidden(false);
    builder
}
