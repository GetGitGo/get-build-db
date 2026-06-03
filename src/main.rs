use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::Path;

// ── Step 1: log → compile_commands ──────────────────────────────────────────

#[derive(Serialize, Clone)]
struct CompileCommand {
    directory: String,
    command: String,
    file: String,
}

fn parse_log(input: &str) -> Vec<CompileCommand> {
    let raw = fs::read(input).expect("Failed to read log file");
    let content = String::from_utf8_lossy(&raw);

    let source_exts: HashSet<&str> =
        ["c", "cpp", "cc", "cxx", "c++", "C", "s", "S", "asm"]
            .iter()
            .cloned()
            .collect();

    let non_compiler_prefixes = [
        "make[", "rm ", "cp ", "mkdir", "test ", "test -", "for ", "do ",
        "if ", "done", "fi", "#", "echo", "cat ", "sed ", "awk ", "grep ",
        "arm-linux-gnueabihf-sigmastar-11.1.0-strip",
    ];

    let mut current_dir = String::new();
    let mut commands: Vec<CompileCommand> = Vec::new();
    let mut skipped_link = 0u32;
    let mut skipped_other = 0u32;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(dir) = extract_entering_dir(trimmed) {
            current_dir = dir;
            continue;
        }
        if is_non_compiler(trimmed, &non_compiler_prefixes) {
            continue;
        }
        if !has_compile_flag(trimmed) {
            if looks_like_compiler(trimmed) {
                skipped_link += 1;
            }
            continue;
        }

        let command = trimmed.to_string();
        if let Some(source) = extract_source(trimmed, &source_exts) {
            commands.push(CompileCommand {
                directory: current_dir.clone(),
                command,
                file: resolve_path(source, &current_dir),
            });
        } else {
            skipped_other += 1;
        }
    }

    eprintln!(
        "Step 1/3: Extracted {} compile commands (skipped: {} link, {} unrecognized)",
        commands.len(),
        skipped_link,
        skipped_other
    );
    commands
}

fn extract_entering_dir(line: &str) -> Option<String> {
    if !line.starts_with("make[") {
        return None;
    }
    let start = line.find('\'')?;
    let rest = &line[start + 1..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn is_non_compiler(line: &str, prefixes: &[&str]) -> bool {
    if line.is_empty() {
        return true;
    }
    for prefix in prefixes {
        if line.starts_with(prefix) {
            return true;
        }
    }
    false
}

fn looks_like_compiler(line: &str) -> bool {
    let first_token = line.split_whitespace().next().unwrap_or("");
    first_token.ends_with("-gcc")
        || first_token.ends_with("-g++")
        || first_token.ends_with("-cc")
        || first_token.ends_with("-clang")
        || first_token.ends_with("-clang++")
        || first_token == "gcc"
        || first_token == "g++"
        || first_token == "cc"
        || first_token == "clang"
        || first_token == "clang++"
}

fn has_compile_flag(line: &str) -> bool {
    line.split_whitespace().any(|t| t == "-c")
}

fn extract_source<'a>(line: &'a str, exts: &HashSet<&str>) -> Option<&'a str> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut candidates = Vec::new();
    for i in 0..tokens.len() {
        let token = tokens[i];
        if token.starts_with('-') {
            continue;
        }
        if i > 0 && tokens[i - 1] == "-o" {
            continue;
        }
        if let Some(ext) = Path::new(token).extension() {
            if exts.contains(ext.to_str().unwrap_or("")) {
                candidates.push(token);
            }
        }
    }
    candidates.last().copied()
}

fn resolve_path(source: &str, dir: &str) -> String {
    let path = Path::new(source);
    if path.is_absolute() {
        return source.to_string();
    }
    normalize_path(&Path::new(dir).join(source))
}

fn normalize_path(path: &Path) -> String {
    use std::path::Component;
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                components.pop();
            }
            Component::CurDir => {}
            c => components.push(c),
        }
    }
    if components.is_empty() {
        return String::from(".");
    }
    let mut result = String::new();
    for (i, c) in components.iter().enumerate() {
        if i > 0 {
            let prev = &components[i - 1];
            if !matches!(prev, Component::RootDir) {
                result.push('/');
            }
        }
        result.push_str(c.as_os_str().to_str().unwrap_or(""));
    }
    result
}

// ── Step 2: compile_commands → dir_tree ─────────────────────────────────────

#[derive(Serialize)]
struct DirTree {
    root: String,
    tree: DirNode,
}

#[derive(Serialize)]
struct DirNode {
    name: String,
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    dirs: BTreeMap<String, DirNode>,
}

fn build_tree(commands: &[CompileCommand]) -> DirTree {
    let mut dir_files: HashMap<String, Vec<String>> = HashMap::new();
    for cmd in commands {
        let dir = Path::new(&cmd.file).parent().unwrap().to_str().unwrap().to_string();
        let filename = Path::new(&cmd.file)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        dir_files.entry(dir).or_default().push(filename);
    }
    for files in dir_files.values_mut() {
        files.sort();
    }

    let all_dirs: Vec<&str> = dir_files.keys().map(|d| d.as_str()).collect();
    let common_root = find_common_root(&all_dirs);

    let root_name = Path::new(&common_root)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("."))
        .to_str()
        .unwrap()
        .to_string();

    let mut root_node = DirNode {
        name: root_name.clone(),
        path: common_root.clone(),
        files: Vec::new(),
        dirs: BTreeMap::new(),
    };

    for (dir_path, files) in &dir_files {
        let rel = pathdiff::diff_paths(dir_path, &common_root)
            .unwrap_or_else(|| Path::new(dir_path).to_path_buf());
        if rel.as_os_str().is_empty() {
            root_node.files.extend(files.iter().cloned());
        } else {
            insert_into_tree(&mut root_node, &rel, &common_root, files);
        }
    }

    eprintln!(
        "Step 2/3: Directory tree built — {} dirs, {} files",
        dir_files.len(),
        commands.len()
    );

    DirTree {
        root: common_root,
        tree: root_node,
    }
}

fn find_common_root(paths: &[&str]) -> String {
    if paths.is_empty() {
        return ".".to_string();
    }
    let components: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| {
            Path::new(p)
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect()
        })
        .collect();
    let mut common = Vec::new();
    let min_len = components.iter().map(|c| c.len()).min().unwrap_or(0);
    for i in 0..min_len {
        let comp = components[0][i];
        if components.iter().all(|c| c[i] == comp) {
            common.push(comp);
        } else {
            break;
        }
    }
    if common.is_empty() {
        return "/".to_string();
    }
    let mut result = String::new();
    for c in &common {
        if !result.is_empty() && !result.ends_with('/') {
            result.push('/');
        }
        result.push_str(c);
    }
    if Path::new(paths[0]).is_absolute() && !result.starts_with('/') {
        result.insert(0, '/');
    }
    result
}

fn insert_into_tree(node: &mut DirNode, rel: &Path, base: &str, files: &[String]) {
    let mut current = node;
    let mut cum_path = base.to_string();
    for comp in rel.components() {
        let name = comp.as_os_str().to_str().unwrap();
        cum_path = Path::new(&cum_path).join(name).to_str().unwrap().to_string();
        current = current
            .dirs
            .entry(name.to_string())
            .or_insert_with(|| DirNode {
                name: name.to_string(),
                path: cum_path.clone(),
                files: Vec::new(),
                dirs: BTreeMap::new(),
            });
    }
    current.files.extend(files.iter().cloned());
    current.files.sort();
    current.files.dedup();
}

mod pathdiff {
    use std::path::{Component, Path, PathBuf};

    pub fn diff_paths(target: &str, base: &str) -> Option<PathBuf> {
        let target = Path::new(target);
        let base = Path::new(base);
        let target_comps: Vec<_> = target.components().collect();
        let base_comps: Vec<_> = base.components().collect();
        let mut i = 0;
        while i < target_comps.len()
            && i < base_comps.len()
            && target_comps[i] == base_comps[i]
        {
            i += 1;
        }
        let mut result = PathBuf::new();
        for _ in i..base_comps.len() {
            result.push("..");
        }
        for comp in &target_comps[i..] {
            match comp {
                Component::Normal(s) => result.push(s),
                Component::RootDir => {}
                _ => result.push(comp.as_os_str()),
            }
        }
        Some(if result.as_os_str().is_empty() {
            PathBuf::from("")
        } else {
            result
        })
    }
}

// ── Step 3: dir_tree → copy_sources.sh ──────────────────────────────────────

fn gen_script(tree: &DirTree) -> String {
    let mut script = String::new();
    let root = &tree.root;
    let root_prefix_len = root.len() + 1;

    script.push_str("#!/usr/bin/env bash\n");
    script.push_str("set -euo pipefail\n\n");
    script.push_str("if [ $# -lt 1 ]; then\n");
    script.push_str("    echo \"Usage: $0 <new_dir_name>\"\n");
    script.push_str("    exit 1\n");
    script.push_str("fi\n\n");
    script.push_str("NEW_DIR=\"$1\"\n\n");
    script.push_str(&format!("# Source root: {}\n", root));
    script.push_str("mkdir -p \"$NEW_DIR\"\n\n");
    script.push_str("# ---- Create directory structure ----\n");

    collect_dirs(&tree.tree, root, root_prefix_len, &mut script);

    script.push_str("\n# ---- Copy source files ----\n");

    let mut file_count = 0u32;
    collect_copies(&tree.tree, root, root_prefix_len, &mut script, &mut file_count);

    script.push_str(&format!(
        "\necho \"Copied {} files into $NEW_DIR\"\n",
        file_count
    ));
    script.push_str("TARBALL=\"${NEW_DIR}.tgz\"\n");
    script.push_str(
        "echo \"Creating $TARBALL ...\"\n",
    );
    script.push_str(
        "tar czf \"$TARBALL\" \"$NEW_DIR\"\n",
    );
    script.push_str(
        "echo \"Done. $TARBALL created.\"\n",
    );

    eprintln!("Step 3/3: Copy script generated — {} files", file_count);
    script
}

fn collect_dirs(node: &DirNode, root: &str, prefix_len: usize, script: &mut String) {
    let rel = if node.path == root {
        ""
    } else {
        &node.path[prefix_len..]
    };
    if !rel.is_empty() {
        script.push_str(&format!("mkdir -p \"$NEW_DIR/{}\"\n", rel));
    }
    for (_name, child) in &node.dirs {
        collect_dirs(child, root, prefix_len, script);
    }
}

fn collect_copies(
    node: &DirNode,
    root: &str,
    prefix_len: usize,
    script: &mut String,
    count: &mut u32,
) {
    let rel = if node.path == root {
        ""
    } else {
        &node.path[prefix_len..]
    };
    for file in &node.files {
        let src = format!("{}/{}", node.path, file);
        let dst = if rel.is_empty() {
            format!("\"$NEW_DIR\"/{}", file)
        } else {
            format!("\"$NEW_DIR\"/{}/{}", rel, file)
        };
        script.push_str(&format!(
            "if [ -f \"{}\" ]; then cp \"{}\" {}; else echo \"  SKIP: {}\"; fi\n",
            src, src, dst, src
        ));
        *count += 1;
    }
    for (_name, child) in &node.dirs {
        collect_copies(child, root, prefix_len, script, count);
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <input.log>", args[0]);
        eprintln!("  Generates: compile_commands.json, dir_tree.json, copy_sources.sh");
        std::process::exit(1);
    }

    let input = &args[1];

    // Step 1
    let commands = parse_log(input);
    let cc_json = serde_json::to_string_pretty(&commands).expect("Serialize failed");
    fs::write("compile_commands.json", &cc_json).expect("write compile_commands.json");
    eprintln!("  → compile_commands.json\n");

    // Step 2
    let tree = build_tree(&commands);
    let tree_json = serde_json::to_string_pretty(&tree).expect("Serialize failed");
    fs::write("dir_tree.json", &tree_json).expect("write dir_tree.json");
    eprintln!("  → dir_tree.json\n");

    // Step 3
    let script = gen_script(&tree);
    fs::write("copy_sources.sh", &script).expect("write copy_sources.sh");
    eprintln!("  → copy_sources.sh\n");

    eprintln!("All done.");
}
