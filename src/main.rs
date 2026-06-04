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
        "Step 1/4: Extracted {} compile commands (skipped: {} link, {} unrecognized)",
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
    if unix_path::is_absolute(source) {
        unix_path::normalize(source)
    } else {
        unix_path::join(dir, source)
    }
}

mod unix_path {
    pub fn is_absolute(path: &str) -> bool {
        path.starts_with('/')
    }

    pub fn normalize(path: &str) -> String {
        let is_abs = path.starts_with('/');
        let mut stack: Vec<&str> = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if stack.is_empty() {
                        if !is_abs {
                            stack.push("..");
                        }
                    } else if stack.last() == Some(&"..") {
                        stack.push("..");
                    } else {
                        stack.pop();
                    }
                }
                _ => stack.push(part),
            }
        }
        if stack.is_empty() {
            return if is_abs { "/".to_string() } else { ".".to_string() };
        }
        let body = stack.join("/");
        if is_abs {
            format!("/{body}")
        } else {
            body
        }
    }

    pub fn join(base: &str, rel: &str) -> String {
        if rel.is_empty() {
            return normalize(base);
        }
        if is_absolute(rel) {
            return normalize(rel);
        }
        normalize(&format!("{}/{}", base.trim_end_matches('/'), rel))
    }

    pub fn dirname(path: &str) -> String {
        let path = path.trim_end_matches('/');
        if let Some(pos) = path.rfind('/') {
            if pos == 0 {
                "/".to_string()
            } else {
                path[..pos].to_string()
            }
        } else {
            ".".to_string()
        }
    }

    pub fn basename(path: &str) -> String {
        let path = path.trim_end_matches('/');
        path.rsplit('/').next().unwrap_or(path).to_string()
    }

    fn split_parts(path: &str) -> Vec<&str> {
        path.split('/').filter(|p| !p.is_empty()).collect()
    }

    pub fn relative_parts(target: &str, base: &str) -> Vec<String> {
        let target_parts = split_parts(target);
        let base_parts = split_parts(base);
        let mut i = 0;
        while i < target_parts.len() && i < base_parts.len() && target_parts[i] == base_parts[i] {
            i += 1;
        }
        target_parts[i..].iter().map(|s| s.to_string()).collect()
    }

    pub fn common_prefix(paths: &[&str]) -> String {
        if paths.is_empty() {
            return ".".to_string();
        }
        let components: Vec<Vec<&str>> = paths.iter().map(|p| split_parts(p)).collect();
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
            return if paths[0].starts_with('/') {
                "/".to_string()
            } else {
                ".".to_string()
            };
        }
        let body = common.join("/");
        if paths[0].starts_with('/') {
            format!("/{body}")
        } else {
            body
        }
    }
}

// ── Step 2: compile_commands → cpp_dir_tree ─────────────────────────────────

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

fn build_cpp_tree(commands: &[CompileCommand]) -> DirTree {
    let mut dir_files: HashMap<String, Vec<String>> = HashMap::new();
    for cmd in commands {
        let dir = unix_path::dirname(&cmd.file);
        let filename = unix_path::basename(&cmd.file);
        dir_files.entry(dir).or_default().push(filename);
    }
    for files in dir_files.values_mut() {
        files.sort();
    }

    let all_dirs: Vec<&str> = dir_files.keys().map(|d| d.as_str()).collect();
    let common_root = unix_path::common_prefix(&all_dirs);

    let root_name = unix_path::basename(&common_root);

    let mut root_node = DirNode {
        name: root_name.clone(),
        path: common_root.clone(),
        files: Vec::new(),
        dirs: BTreeMap::new(),
    };

    for (dir_path, files) in &dir_files {
        let rel_parts = unix_path::relative_parts(dir_path, &common_root);
        if rel_parts.is_empty() {
            root_node.files.extend(files.iter().cloned());
        } else {
            insert_into_tree(&mut root_node, &rel_parts, &common_root, files);
        }
    }

    eprintln!(
        "Step 2/4: C/C++ directory tree built — {} dirs, {} files",
        dir_files.len(),
        commands.len()
    );

    DirTree {
        root: common_root,
        tree: root_node,
    }
}

fn insert_into_tree(node: &mut DirNode, rel_parts: &[String], base: &str, files: &[String]) {
    let mut current = node;
    let mut cum_path = base.to_string();
    for name in rel_parts {
        cum_path = unix_path::join(&cum_path, name);
        current = current
            .dirs
            .entry(name.clone())
            .or_insert_with(|| DirNode {
                name: name.clone(),
                path: cum_path.clone(),
                files: Vec::new(),
                dirs: BTreeMap::new(),
            });
    }
    current.files.extend(files.iter().cloned());
    current.files.sort();
    current.files.dedup();
}

// ── Step 3: compile_commands → inc_dir_tree ─────────────────────────────────

fn extract_include_dirs(command: &str, directory: &str) -> Vec<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if token == "-I" || token == "-isystem" {
            i += 1;
            if i < tokens.len() {
                if let Some(path) = resolve_include_path(tokens[i], directory) {
                    result.push(path);
                }
            }
        } else if let Some(path) = token.strip_prefix("-I") {
            if !path.is_empty() {
                if let Some(p) = resolve_include_path(path, directory) {
                    result.push(p);
                }
            }
        } else if let Some(path) = token.strip_prefix("-isystem") {
            if !path.is_empty() {
                if let Some(p) = resolve_include_path(path, directory) {
                    result.push(p);
                }
            }
        }
        i += 1;
    }
    result
}

fn resolve_include_path(path: &str, directory: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    Some(if unix_path::is_absolute(path) {
        unix_path::normalize(path)
    } else {
        unix_path::join(directory, path)
    })
}

fn build_inc_tree(commands: &[CompileCommand]) -> DirTree {
    let mut include_dirs: HashSet<String> = HashSet::new();
    for cmd in commands {
        for inc in extract_include_dirs(&cmd.command, &cmd.directory) {
            include_dirs.insert(inc);
        }
    }

    let all_dirs: Vec<&str> = include_dirs.iter().map(|d| d.as_str()).collect();
    let common_root = unix_path::common_prefix(&all_dirs);

    let root_name = unix_path::basename(&common_root);

    let mut root_node = DirNode {
        name: root_name.clone(),
        path: common_root.clone(),
        files: Vec::new(),
        dirs: BTreeMap::new(),
    };

    for dir_path in &include_dirs {
        let rel_parts = unix_path::relative_parts(dir_path, &common_root);
        if !rel_parts.is_empty() {
            insert_into_tree(&mut root_node, &rel_parts, &common_root, &[]);
        }
    }

    eprintln!(
        "Step 3/4: Include directory tree built — {} unique -I/-isystem paths",
        include_dirs.len()
    );

    DirTree {
        root: common_root,
        tree: root_node,
    }
}

// ── Step 4: cpp_dir_tree + inc_dir_tree → copy_sources.sh ───────────────────

fn gen_script(cpp_tree: &DirTree, inc_tree: &DirTree) -> String {
    let mut script = String::new();
    let root = &cpp_tree.root;
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
    script.push_str("# ---- Create directory structure (C/C++ sources) ----\n");

    collect_dirs(&cpp_tree.tree, root, root_prefix_len, &mut script);

    let mut cpp_paths: HashSet<String> = HashSet::new();
    collect_all_dir_paths(&cpp_tree.tree, &mut cpp_paths);

    script.push_str("\n# ---- Create extra include directories ----\n");
    collect_extra_dirs(
        &inc_tree.tree,
        &cpp_paths,
        root,
        root_prefix_len,
        &mut script,
    );

    script.push_str("\n# ---- Copy source files ----\n");

    let mut source_count = 0u32;
    collect_copies(
        &cpp_tree.tree,
        root,
        root_prefix_len,
        &mut script,
        &mut source_count,
    );

    script.push_str("\n# ---- Copy header files ----\n");

    let mut header_dir_count = 0u32;
    collect_header_copies(
        &inc_tree.tree,
        root,
        root_prefix_len,
        &mut script,
        &mut header_dir_count,
    );

    script.push_str(&format!(
        "\necho \"Copied {} source files + headers from {} include dirs into $NEW_DIR\"\n",
        source_count, header_dir_count
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

    eprintln!(
        "Step 4/4: Copy script generated — {} source files, {} include dirs for headers",
        source_count, header_dir_count
    );
    script
}

fn collect_all_dir_paths(node: &DirNode, paths: &mut HashSet<String>) {
    paths.insert(node.path.clone());
    for child in node.dirs.values() {
        collect_all_dir_paths(child, paths);
    }
}

fn collect_extra_dirs(
    node: &DirNode,
    cpp_paths: &HashSet<String>,
    root: &str,
    prefix_len: usize,
    script: &mut String,
) {
    if node.path != root && !cpp_paths.contains(&node.path) {
        let rel = &node.path[prefix_len..];
        script.push_str(&format!("mkdir -p \"$NEW_DIR/{}\"\n", rel));
    }
    for child in node.dirs.values() {
        collect_extra_dirs(child, cpp_paths, root, prefix_len, script);
    }
}

fn collect_header_copies(
    node: &DirNode,
    root: &str,
    prefix_len: usize,
    script: &mut String,
    dir_count: &mut u32,
) {
    if node.path != root {
        let rel = &node.path[prefix_len..];
        script.push_str(&format!("if [ -d \"{}\" ]; then\n", node.path));
        script.push_str(&format!(
            "  for hdr in \"{}\"/*.h \"{}\"/*.hpp; do\n",
            node.path, node.path
        ));
        script.push_str("    [ -f \"$hdr\" ] || continue\n");
        script.push_str(&format!("    cp \"$hdr\" \"$NEW_DIR/{}/\"\n", rel));
        script.push_str("  done\n");
        script.push_str("fi\n");
        *dir_count += 1;
    }

    for child in node.dirs.values() {
        collect_header_copies(child, root, prefix_len, script, dir_count);
    }
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

struct OutputNames {
    prefix: String,
    compile_commands: String,
    cpp_dir_tree: String,
    inc_dir_tree: String,
    copy_sources: String,
}

/// 取参数文件名（去扩展名）按 `_` 或 `-` 分割的第一节作为输出前缀。
fn output_names_from_input(input: &str) -> OutputNames {
    let stem = Path::new(input)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let prefix = stem
        .split(|c: char| c == '_' || c == '-')
        .next()
        .unwrap_or(stem)
        .to_string();
    OutputNames {
        compile_commands: format!("{prefix}_compile_commands.json"),
        cpp_dir_tree: format!("{prefix}_cpp_dir_tree.json"),
        inc_dir_tree: format!("{prefix}_inc_dir_tree.json"),
        copy_sources: format!("{prefix}_copy_sources.sh"),
        prefix,
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <input.log>", args[0]);
        eprintln!(
            "  Generates: <prefix>_compile_commands.json, <prefix>_cpp_dir_tree.json, ..."
        );
        eprintln!("  <prefix> = input filename stem before first '_' or '-'");
        std::process::exit(1);
    }

    let input = &args[1];
    let out = output_names_from_input(input);
    eprintln!("Output prefix: {}\n", out.prefix);

    // Step 1
    let commands = parse_log(input);
    let cc_json = serde_json::to_string_pretty(&commands).expect("Serialize failed");
    fs::write(&out.compile_commands, &cc_json)
        .unwrap_or_else(|e| panic!("write {}: {e}", out.compile_commands));
    eprintln!("  → {}\n", out.compile_commands);

    // Step 2
    let cpp_tree = build_cpp_tree(&commands);
    let cpp_tree_json = serde_json::to_string_pretty(&cpp_tree).expect("Serialize failed");
    fs::write(&out.cpp_dir_tree, &cpp_tree_json)
        .unwrap_or_else(|e| panic!("write {}: {e}", out.cpp_dir_tree));
    eprintln!("  → {}\n", out.cpp_dir_tree);

    // Step 3
    let inc_tree = build_inc_tree(&commands);
    let inc_tree_json = serde_json::to_string_pretty(&inc_tree).expect("Serialize failed");
    fs::write(&out.inc_dir_tree, &inc_tree_json)
        .unwrap_or_else(|e| panic!("write {}: {e}", out.inc_dir_tree));
    eprintln!("  → {}\n", out.inc_dir_tree);

    // Step 4
    let script = gen_script(&cpp_tree, &inc_tree);
    fs::write(&out.copy_sources, &script)
        .unwrap_or_else(|e| panic!("write {}: {e}", out.copy_sources));
    eprintln!("  → {}\n", out.copy_sources);

    eprintln!("All done.");
}
