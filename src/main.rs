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

fn read_log(input: &str) -> String {
    let raw = fs::read(input).unwrap_or_else(|e| panic!("read log {}: {e}", input));
    String::from_utf8_lossy(&raw).into_owned()
}

fn parse_log_content(content: &str, project_root: &str) -> Vec<CompileCommand> {
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
            let directory =
                infer_compile_directory(&command, project_root, &current_dir);
            let file = resolve_path(source, &directory);
            commands.push(CompileCommand {
                directory,
                command,
                file,
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

fn extract_o_output(command: &str) -> Option<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    for i in 0..tokens.len() {
        if tokens[i] == "-o" && i + 1 < tokens.len() {
            return Some(tokens[i + 1].to_string());
        }
    }
    None
}

fn count_leading_dotdot(path: &str) -> usize {
    path.split('/').take_while(|p| *p == "..").count()
}

fn strip_leading_dotdot(path: &str) -> String {
    path.split('/')
        .skip_while(|p| *p == "..")
        .collect::<Vec<_>>()
        .join("/")
}

/// 由相对路径在 project_root 下的解析结果，反推编译工作目录（绝对路径）。
fn infer_compile_directory(command: &str, project_root: &str, current_dir: &str) -> String {
    if !current_dir.is_empty() {
        return unix_path::normalize(current_dir);
    }

    let source_exts: HashSet<&str> = ["c", "cpp", "cc", "cxx", "c++", "C"]
        .iter()
        .cloned()
        .collect();
    let rel = extract_o_output(command)
        .or_else(|| extract_source(command, &source_exts).map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string());

    if unix_path::is_absolute(&rel) {
        return unix_path::dirname(&rel);
    }

    let tail = strip_leading_dotdot(&rel);
    let abs = if tail.is_empty() {
        unix_path::normalize(project_root)
    } else {
        unix_path::join(project_root, &tail)
    };

    let abs_parts: Vec<&str> = abs
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    let rel_parts: Vec<&str> = rel
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let ups = count_leading_dotdot(&rel);
    let cwd_len = abs_parts
        .len()
        .saturating_sub(rel_parts.len())
        .saturating_add(2 * ups);

    if cwd_len == 0 {
        return "/".to_string();
    }
    format!("/{}", abs_parts[..cwd_len].join("/"))
}

fn extract_project_root_from_log(content: &str) -> String {
    let mut paths: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !looks_like_compiler(trimmed) {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i] == "-I" || tokens[i] == "-isystem" {
                i += 1;
                if i < tokens.len() && unix_path::is_absolute(tokens[i]) {
                    paths.push(unix_path::normalize(tokens[i]));
                }
            } else if let Some(p) = tokens[i].strip_prefix("-I") {
                if unix_path::is_absolute(p) {
                    paths.push(unix_path::normalize(p));
                }
            } else if let Some(p) = tokens[i].strip_prefix("-isystem") {
                if unix_path::is_absolute(p) {
                    paths.push(unix_path::normalize(p));
                }
            }
            i += 1;
        }
    }
    if paths.is_empty() {
        return ".".to_string();
    }
    let refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    unix_path::common_prefix(&refs)
}

/// 节点绝对路径相对 project_root 的子路径（用于 $NEW_DIR 下布局）。
fn rel_under_root(node_path: &str, root: &str) -> String {
    if node_path == root {
        return String::new();
    }
    if root == "/" {
        return node_path.trim_start_matches('/').to_string();
    }
    let prefix = format!("{}/", root.trim_end_matches('/'));
    node_path
        .strip_prefix(&prefix)
        .unwrap_or(node_path)
        .to_string()
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

struct DepBlock {
    object: String,
    deps: Vec<String>,
}

fn parse_dep_log(content: &str) -> Vec<DepBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<DepBlock> = None;

    for line in content.lines() {
        if is_phony_target_line(line) {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some((object, rest)) = parse_dep_rule_start(line) {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(DepBlock {
                object,
                deps: tokenize_dep_tokens(&rest),
            });
            continue;
        }

        if is_dep_continuation(line) {
            if let Some(block) = current.as_mut() {
                block.deps.extend(tokenize_dep_tokens(trimmed));
            }
        }
    }

    if let Some(block) = current {
        blocks.push(block);
    }

    blocks
}

fn parse_dep_rule_start(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let idx = trimmed.find(".o:")?;
    let object = trimmed[..idx + 2].trim().to_string();
    if !object.ends_with(".o") || object.contains(' ') {
        return None;
    }
    let rest = trimmed[idx + 3..].trim();
    Some((object, rest.to_string()))
}

fn is_dep_continuation(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

/// `-MP` 产生的空目标行，如 `/path/foo.h:`
fn is_phony_target_line(line: &str) -> bool {
    let t = line.trim();
    if !t.ends_with(':') || t.contains(".o:") {
        return false;
    }
    let head = t.trim_end_matches(':').trim();
    head.ends_with(".h")
        || head.ends_with(".hpp")
        || head.ends_with(".hh")
        || head.contains("/include/")
        || head.starts_with("/usr/")
        || head.starts_with("/opt/")
}

fn tokenize_dep_tokens(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|t| t.trim_end_matches('\\').trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn collect_isystem_dirs(commands: &[CompileCommand]) -> HashSet<String> {
    let mut dirs = HashSet::new();
    for cmd in commands {
        let tokens: Vec<&str> = cmd.command.split_whitespace().collect();
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i] == "-isystem" {
                i += 1;
                if i < tokens.len() {
                    if let Some(p) = resolve_include_path(tokens[i], &cmd.directory) {
                        dirs.insert(p);
                    }
                }
            } else if let Some(path) = tokens[i].strip_prefix("-isystem") {
                if !path.is_empty() {
                    if let Some(p) = resolve_include_path(path, &cmd.directory) {
                        dirs.insert(p);
                    }
                }
            }
            i += 1;
        }
    }
    dirs
}

fn is_system_header(path: &str, isystem_dirs: &HashSet<String>) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return true;
    }
    for d in isystem_dirs {
        if path.starts_with(d) {
            return true;
        }
    }
    const PREFIXES: &[&str] = &[
        "/usr/include",
        "/usr/local/include",
        "/opt/gcc-",
        "/opt/gcc/",
        "/lib/gcc/",
        "/include/c++/",
    ];
    if PREFIXES.iter().any(|p| path.starts_with(p)) {
        return true;
    }
    if path.contains("/libc/usr/include") || path.contains("/lib/clang/") {
        return true;
    }
    false
}

fn is_header_file(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(ext, "h" | "hpp" | "hh" | "H" | "hxx" | "h++")
}

fn find_compile_for_object<'a>(
    object: &str,
    commands: &'a [CompileCommand],
) -> Option<&'a CompileCommand> {
    let object_base = unix_path::basename(object);
    commands.iter().find(|cmd| {
        cmd.command.split_whitespace().any(|t| {
            t == object
                || t.ends_with(&format!("/{object}"))
                || t == object_base
                || t.ends_with(&format!("/{object_base}"))
        })
    })
}

fn resolve_dep_path(raw: &str, base_dir: &str) -> String {
    if unix_path::is_absolute(raw) {
        unix_path::normalize(raw)
    } else {
        unix_path::join(base_dir, raw)
    }
}

fn collect_headers_from_dep_log(
    content: &str,
    commands: &[CompileCommand],
    isystem_dirs: &HashSet<String>,
) -> HashMap<String, Vec<String>> {
    let mut dir_files: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen_headers: HashSet<String> = HashSet::new();
    let mut skipped_system = 0u32;

    for block in parse_dep_log(content) {
        let base_dir = find_compile_for_object(&block.object, commands)
            .map(|c| c.directory.as_str())
            .unwrap_or(".");

        for raw in &block.deps {
            if raw == &block.object {
                continue;
            }
            let lower = raw.to_ascii_lowercase();
            if lower.ends_with(".o") || lower.ends_with(".cpp") || lower.ends_with(".cc") || lower.ends_with(".c") {
                continue;
            }
            if !is_header_file(raw) {
                continue;
            }

            let abs = resolve_dep_path(raw, base_dir);
            if is_system_header(&abs, isystem_dirs) {
                skipped_system += 1;
                continue;
            }
            if !seen_headers.insert(abs.clone()) {
                continue;
            }

            let dir = unix_path::dirname(&abs);
            let name = unix_path::basename(&abs);
            dir_files.entry(dir).or_default().push(name);
        }
    }

    for files in dir_files.values_mut() {
        files.sort();
        files.dedup();
    }

    eprintln!(
        "  dep log: {} header paths (skipped {} system/stdlib)",
        seen_headers.len(),
        skipped_system
    );

    dir_files
}

fn build_inc_tree(
    commands: &[CompileCommand],
    dep_log_content: &str,
    project_root: &str,
) -> DirTree {
    let mut include_dirs: HashSet<String> = HashSet::new();
    for cmd in commands {
        for inc in extract_include_dirs(&cmd.command, &cmd.directory) {
            include_dirs.insert(inc);
        }
    }

    let isystem_dirs = collect_isystem_dirs(commands);
    let header_dir_files = collect_headers_from_dep_log(dep_log_content, commands, &isystem_dirs);

    for dir in header_dir_files.keys() {
        include_dirs.insert(dir.clone());
    }

    let all_dirs: Vec<String> = include_dirs.into_iter().collect();
    let common_root = unix_path::normalize(project_root);
    let root_name = unix_path::basename(&common_root);

    let mut root_node = DirNode {
        name: root_name.clone(),
        path: common_root.clone(),
        files: Vec::new(),
        dirs: BTreeMap::new(),
    };

    for dir_path in &all_dirs {
        let files = header_dir_files.get(dir_path).map(|v| v.as_slice()).unwrap_or(&[]);
        let rel_parts = unix_path::relative_parts(dir_path, &common_root);
        if rel_parts.is_empty() {
            if !files.is_empty() {
                root_node.files.extend(files.iter().cloned());
                root_node.files.sort();
                root_node.files.dedup();
            }
        } else {
            insert_into_tree(&mut root_node, &rel_parts, &common_root, files);
        }
    }

    let header_count: usize = header_dir_files.values().map(|v| v.len()).sum();
    let dirs_with_headers = header_dir_files.len();

    eprintln!(
        "Step 3/4: Include directory tree — {} dirs, {} headers in {} directories",
        all_dirs.len(),
        header_count,
        dirs_with_headers
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

    collect_dirs(&cpp_tree.tree, root, &mut script);

    let mut cpp_paths: HashSet<String> = HashSet::new();
    collect_all_dir_paths(&cpp_tree.tree, &mut cpp_paths);

    script.push_str("\n# ---- Create extra include directories ----\n");
    collect_extra_dirs(&inc_tree.tree, &cpp_paths, root, &mut script);

    script.push_str("\n# ---- Copy source files ----\n");

    let mut source_count = 0u32;
    collect_copies(&cpp_tree.tree, root, &mut script, &mut source_count);

    script.push_str("\n# ---- Copy header files ----\n");

    let mut header_count = 0u32;
    collect_header_copies(&inc_tree.tree, root, &mut script, &mut header_count);

    script.push_str(&format!(
        "\necho \"Copied {} source + {} header files into $NEW_DIR\"\n",
        source_count, header_count
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
        "Step 4/4: Copy script generated — {} source + {} header files",
        source_count, header_count
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
    script: &mut String,
) {
    if node.path != root && !cpp_paths.contains(&node.path) {
        let rel = rel_under_root(&node.path, root);
        script.push_str(&format!("mkdir -p \"$NEW_DIR/{}\"\n", rel));
    }
    for child in node.dirs.values() {
        collect_extra_dirs(child, cpp_paths, root, script);
    }
}

fn collect_header_copies(node: &DirNode, root: &str, script: &mut String, count: &mut u32) {
    let rel = rel_under_root(&node.path, root);
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
    for child in node.dirs.values() {
        collect_header_copies(child, root, script, count);
    }
}

fn collect_dirs(node: &DirNode, root: &str, script: &mut String) {
    let rel = rel_under_root(&node.path, root);
    if !rel.is_empty() {
        script.push_str(&format!("mkdir -p \"$NEW_DIR/{}\"\n", rel));
    }
    for (_name, child) in &node.dirs {
        collect_dirs(child, root, script);
    }
}

fn collect_copies(node: &DirNode, root: &str, script: &mut String, count: &mut u32) {
    let rel = rel_under_root(&node.path, root);
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
        collect_copies(child, root, script, count);
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
    if args.len() < 3 {
        eprintln!("Usage: {} <stdout.log> <depends.log>", args[0]);
        eprintln!(
            "  Generates: <prefix>_compile_commands.json, <prefix>_cpp_dir_tree.json, ..."
        );
        eprintln!("  <prefix> = stdout.log filename stem before first '_' or '-'");
        eprintln!("  stdout.log  — 完整 make 过程（含 Entering directory、编译命令）");
        eprintln!("  depends.log — Makefile -MD 依赖块（见 README）");
        std::process::exit(1);
    }

    let stdout_log = &args[1];
    let depends_log = &args[2];
    let out = output_names_from_input(stdout_log);
    eprintln!("Output prefix: {}\n", out.prefix);

    let stdout_content = read_log(stdout_log);
    let depends_content = read_log(depends_log);
    let project_root = extract_project_root_from_log(&stdout_content);
    eprintln!("Project root: {}\n", project_root);

    // Step 1
    let commands = parse_log_content(&stdout_content, &project_root);
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
    let inc_tree = build_inc_tree(&commands, &depends_content, &cpp_tree.root);
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DEP: &str = r#"main.o: main.cpp \
 /var/project/app/common/config.h \
 /opt/gcc-11/libc/usr/include/stdio.h \
 ../common/utils.h

/var/project/app/common/config.h:

encoder.o: encoder.cpp /var/project/app/common/foo.hpp
"#;

    #[test]
    fn parse_dep_blocks() {
        let blocks = parse_dep_log(SAMPLE_DEP);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].object, "main.o");
        assert!(blocks[0].deps.iter().any(|d| d.contains("config.h")));
        assert_eq!(blocks[1].object, "encoder.o");
    }

    #[test]
    fn filters_system_headers() {
        let isystem = HashSet::new();
        let commands = vec![CompileCommand {
            directory: "/var/project/app/udg".to_string(),
            command: "g++ -c -o main.o main.cpp".to_string(),
            file: "/var/project/app/udg/main.cpp".to_string(),
        }];
        let mut cmds = commands;
        for c in &mut cmds {
            c.directory = "/var/project/app/udg".to_string();
        }
        let dir_files = collect_headers_from_dep_log(SAMPLE_DEP, &cmds, &isystem);
        assert!(!dir_files.keys().any(|k| k.contains("/opt/gcc")));
        assert!(!dir_files.keys().any(|k| k.contains("stdio")));
        assert!(dir_files.keys().any(|k| k.contains("common")));
    }
}
