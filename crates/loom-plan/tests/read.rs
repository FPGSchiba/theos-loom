//! Behaviour tests of `read`: a whole planning repo into one `PlanningRepo`.

use std::fs;
use std::path::{Path, PathBuf};

use loom_plan::model::{DirectiveFile, DocStatus, LeafKind, LoomMark, TicketId};
use loom_plan::{Component, Epic, Finding, PlanningRepo, ReadError, Rule, Severity, Story, read};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CART: &str = "components/app/initiative-checkout/tickets/epic-cart";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn valid() -> PathBuf {
    fixture("valid")
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A copy of `valid/` in a temporary folder.
fn valid_copy() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("repo");
    copy_dir(&valid(), &root)?;
    Ok((dir, root))
}

fn write(root: &Path, path: &str, text: &str) -> std::io::Result<()> {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

const LOOM_TOML: &str = include_str!("fixtures/valid/loom.toml");
const COMPONENT: &str = "format = 1\n[component]\nid = \"app\"\nname = \"Web shop\"\nrepo = \"r\"\ndefault_branch = \"main\"\n";
const INITIATIVE: &str = "---\ntype: initiative\ntitle: \"Shoppers check out\"\n---\n# T\n";

/// A small tree: one component `app` with the initiative `initiative-x` and `files` added.
fn tree(
    files: &[(&str, &str)],
) -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("repo");
    write(&root, "loom.toml", LOOM_TOML)?;
    write(&root, "components/app/component.toml", COMPONENT)?;
    write(
        &root,
        "components/app/initiative-x/initiative-x.md",
        INITIATIVE,
    )?;
    for (path, text) in files {
        write(&root, path, text)?;
    }
    Ok((dir, root))
}

fn component<'a>(repo: &'a PlanningRepo, id: &str) -> Result<&'a Component, String> {
    repo.components
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("no component {id}"))
}

fn epic<'a>(component: &'a Component, slug: &str) -> Result<&'a Epic, String> {
    let initiative = component.initiative.as_ref().ok_or("no initiative")?;
    initiative
        .children
        .iter()
        .chain(std::iter::once(initiative))
        .flat_map(|i| i.epics.iter())
        .find(|e| e.slug == slug)
        .ok_or_else(|| format!("no epic {slug}"))
}

fn names(file: Option<&DirectiveFile>) -> Vec<String> {
    file.map(|f| f.directives.iter().filter_map(|d| d.name.clone()).collect())
        .unwrap_or_default()
}

fn key(story: &Story) -> Option<String> {
    story.key().map(ToString::to_string)
}

fn ids(story: &Story) -> Vec<String> {
    story
        .criterion_ids()
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn only_finding(repo: &PlanningRepo) -> Result<&Finding, String> {
    match repo.read_findings.as_slice() {
        [one] => Ok(one),
        other => Err(format!("expected one finding, got {other:#?}")),
    }
}

// AC: TU-01.1
#[test]
fn every_file_of_the_valid_tree_is_read_as_its_model() -> TestResult {
    let repo = read(&valid())?;
    assert!(repo.read_findings.is_empty(), "{:#?}", repo.read_findings);
    assert_eq!(repo.root, valid());

    let settings = repo.settings.as_ref().ok_or("no loom.toml")?;
    assert_eq!(settings.project.id, "shop");
    let vision = repo.vision.as_ref().ok_or("no vision")?;
    assert_eq!(
        vision.front.as_ref().and_then(|f| f.status),
        Some(DocStatus::Draft)
    );
    assert_eq!(
        names(repo.project_directives.as_ref()),
        ["Small modules", "Tests first", "Plain language"]
    );

    let ids_of: Vec<&str> = repo.components.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids_of, ["api", "app"]);

    // The app component: a parent initiative with one child, epics in every state, a backlog.
    let app = component(&repo, "app")?;
    assert_eq!(
        app.settings.as_ref().map(|s| s.component.id.as_str()),
        Some("app")
    );
    assert_eq!(names(app.engineering.as_ref()), ["Money is typed"]);
    assert_eq!(names(app.design.as_ref()), ["One primary button"]);
    assert_eq!(app.extra_initiatives.len(), 0);
    let parent = app.initiative.as_ref().ok_or("no initiative")?;
    assert_eq!(parent.slug, "initiative-shop");
    assert_eq!(
        parent.breakdown.as_ref().map(|b| b.initiative.len()),
        Some(1)
    );
    let [child] = parent.children.as_slice() else {
        return Err("expected one child initiative".into());
    };
    assert_eq!(child.slug, "initiative-checkout");
    let child_file = child.file.as_ref().ok_or("no child file")?;
    assert_eq!(child_file.front.parent.as_deref(), Some("initiative-shop"));
    assert_eq!(child_file.requirements.len(), 2);
    let slugs: Vec<&str> = child.epics.iter().map(|e| e.slug.as_str()).collect();
    assert_eq!(
        slugs,
        [
            "epic-cart",
            "epic-search",
            "epic-wishlist",
            "epic-gift-cards"
        ]
    );

    let cart = epic(app, "epic-cart")?;
    assert_eq!(cart.path, CART);
    assert_eq!(
        cart.key().map(ToString::to_string).as_deref(),
        Some("APP-1")
    );
    assert_eq!(cart.loom(), Some(LoomMark::Ready));
    assert_eq!(cart.row.as_ref().map(|r| r.id.as_str()), Some("1"));
    let body = cart.body.as_ref().ok_or("no body")?;
    assert!(body.goal_present && body.scope_present);
    assert_eq!(body.requirements.len(), 2);

    let search = epic(app, "epic-search")?;
    assert_eq!(search.loom(), None);
    assert!(!search.body.as_ref().is_some_and(|b| b.goal_present));
    assert_eq!(epic(app, "epic-wishlist")?.loom(), Some(LoomMark::Paused));
    assert!(epic(app, "epic-wishlist")?.stories.is_empty());
    assert!(epic(app, "epic-gift-cards")?.is_dropped());
    assert!(!cart.is_dropped());

    assert_valid_stories(app)?;
    assert_valid_api_and_files(&repo)
}

/// The stories of the valid tree: files, a plan, entries only, a backlog bug.
fn assert_valid_stories(app: &Component) -> TestResult {
    let cart = epic(app, "epic-cart")?;
    let search = epic(app, "epic-search")?;
    let [add, spike] = cart.stories.as_slice() else {
        return Err("expected two stories in epic-cart".into());
    };
    assert_eq!(add.kind, LeafKind::Story);
    assert_eq!(key(add).as_deref(), Some("APP-2"));
    assert_eq!(ids(add), ["APP-2.1", "APP-2.2"]);
    assert_eq!(
        add.plan.as_ref().map(|p| p.path.as_str()),
        Some(format!("{CART}/tickets/story-add-item-plan.md").as_str())
    );
    let needs: Vec<String> = add.needs().iter().map(ToString::to_string).collect();
    assert_eq!(needs, ["API-2"]);
    assert_eq!(spike.kind, LeafKind::Spike);
    assert_eq!(ids(spike), ["APP-3.1"]);
    assert!(spike.plan.is_none());
    assert_eq!(spike.after().len(), 1);

    let [entry_only] = search.stories.as_slice() else {
        return Err("expected one story in epic-search".into());
    };
    assert!(entry_only.file.is_none() && entry_only.entry.is_some());
    assert_eq!(entry_only.kind, LeafKind::Story);
    assert_eq!(entry_only.criteria().len(), 0);

    let [bug] = app.backlog.as_slice() else {
        return Err("expected one backlog leaf".into());
    };
    assert_eq!(bug.kind, LeafKind::Bug);
    assert_eq!(ids(bug), ["APP-7.1"]);

    Ok(())
}

/// The api component, decisions, state mirrors and documents of the valid tree.
fn assert_valid_api_and_files(repo: &PlanningRepo) -> TestResult {
    // The api component: one initiative without epics, stories directly under it.
    let api = component(repo, "api")?;
    assert_eq!(
        api.settings.as_ref().map(|s| s.component.id.as_str()),
        Some("api")
    );
    assert_eq!(names(api.engineering.as_ref()), ["Idempotent endpoints"]);
    assert!(api.design.is_none());
    let payments = api.initiative.as_ref().ok_or("no api initiative")?;
    assert!(payments.epics.is_empty() && payments.children.is_empty());
    let [charge, refund] = payments.stories.as_slice() else {
        return Err("expected two api stories".into());
    };
    assert_eq!(key(charge).as_deref(), Some("API-2"));
    assert_eq!(charge.loom(), Some(LoomMark::Ready));
    assert!(refund.file.is_none());

    let numbers: Vec<u32> = repo.decisions.iter().map(|d| d.number).collect();
    assert_eq!(numbers, [1, 2]);
    let [state] = repo.state_mirrors.as_slice() else {
        return Err("expected one state mirror".into());
    };
    assert_eq!(state.path, "_state/app/APP-1.toml");
    assert_eq!(
        (state.component.as_str(), state.name.as_str()),
        ("app", "APP-1")
    );
    assert_eq!(state.mirror.stories.len(), 2);

    let docs: Vec<&str> = repo.documents.iter().map(|d| d.path.as_str()).collect();
    for path in [
        "README.md",
        "canon/logo.png",
        "components/app/initiative-checkout/prd-checkout.md",
        "components/app/initiative-shop/.memlog.md",
    ] {
        assert!(docs.contains(&path), "{path} is not a document: {docs:?}");
    }
    assert!(repo.stray_plans.is_empty());
    Ok(())
}

// AC: TU-01.1
#[test]
fn unknown_key_anywhere_in_the_tree_is_reported_with_file_and_key() -> TestResult {
    let (_dir, root) = valid_copy()?;
    let path = format!("{CART}/epic-cart.md");
    let text = fs::read_to_string(root.join(&path))?;
    fs::write(
        root.join(&path),
        text.replacen("---\n", "---\nbogus_key: 1\n", 1),
    )?;

    let repo = read(&root)?;
    let finding = only_finding(&repo)?;
    assert_eq!(finding.rule, Rule::UnknownKey);
    assert_eq!(finding.path, path);
    assert!(finding.message.contains("bogus_key"), "{}", finding.message);

    // Everything else is still read.
    let full = read(&valid())?;
    assert_eq!(repo.documents.len(), full.documents.len());
    assert_eq!(repo.decisions.len(), full.decisions.len());
    let cart = epic(component(&repo, "app")?, "epic-cart")?;
    assert!(cart.file.is_none());
    assert_eq!(cart.stories.len(), 2);
    assert_eq!(
        epic(component(&repo, "app")?, "epic-search")?.stories.len(),
        1
    );
    Ok(())
}

#[test]
fn build_order_is_the_table_order() -> TestResult {
    let repo = read(&valid())?;
    let cart = epic(component(&repo, "app")?, "epic-cart")?;
    let order: Vec<(Option<&str>, usize)> = cart
        .stories
        .iter()
        .map(|s| (s.id.as_ref().map(TicketId::as_str), s.order))
        .collect();
    assert_eq!(order, [(Some("2"), 0), (Some("1"), 1)]);
    Ok(())
}

#[test]
fn file_wins_over_entry() -> TestResult {
    let (_dir, root) = tree(&[
        (
            "components/app/initiative-x/tickets/tickets.toml",
            "[[entry]]\nid = 1\ntype = \"story\"\ntitle = \"Old title\"\nafter = [2]\n\n[[entry]]\nid = 2\ntype = \"story\"\ntitle = \"B\"\n",
        ),
        (
            "components/app/initiative-x/tickets/story-a.md",
            "---\nid: 1\ntype: story\ntitle: \"A\"\nkey: APP-1\nloom: ready\nneeds: [API-1]\n---\n# A\n",
        ),
    ])?;
    let repo = read(&root)?;
    assert!(repo.read_findings.is_empty(), "{:#?}", repo.read_findings);
    let initiative = component(&repo, "app")?
        .initiative
        .as_ref()
        .ok_or("no initiative")?;
    let story = initiative.stories.first().ok_or("no story")?;
    assert!(story.entry.is_some() && story.file.is_some());
    assert_eq!(key(story).as_deref(), Some("APP-1"));
    assert_eq!(story.loom(), Some(LoomMark::Ready));
    assert_eq!(story.needs().len(), 1);
    assert!(story.after().is_empty(), "the file's absent `after` wins");
    Ok(())
}

// AC: TU-01.2
#[test]
fn differing_loom_key_on_file_and_entry_is_drift() -> TestResult {
    let repo = read(&fixture("invalid/drift-loom"))?;
    let finding = only_finding(&repo)?;
    assert_eq!(
        (finding.rule, finding.severity),
        (Rule::Drift, Severity::Error)
    );
    assert_eq!(
        finding.path,
        "components/app/initiative-checkout/tickets/epic-cart/tickets/story-add-item.md"
    );
    assert!(finding.message.contains("`loom`"), "{}", finding.message);
    let story = epic(component(&repo, "app")?, "epic-cart")?
        .stories
        .first()
        .ok_or("no story")?;
    assert_eq!(story.loom(), Some(LoomMark::Paused), "the file counts");
    Ok(())
}

// AC: TU-01.2
#[test]
fn two_top_level_initiatives_are_a_layout_finding() -> TestResult {
    let repo = read(&fixture("invalid/layout-two-initiatives"))?;
    let finding = only_finding(&repo)?;
    assert_eq!(
        (finding.rule, finding.severity),
        (Rule::Layout, Severity::Error)
    );
    assert_eq!(finding.path, "components/app/initiative-search");
    let app = component(&repo, "app")?;
    assert_eq!(
        app.initiative.as_ref().map(|i| i.slug.as_str()),
        Some("initiative-cart")
    );
    assert_eq!(app.extra_initiatives, ["initiative-search"]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn component_id_not_its_folder_is_a_layout_finding() -> TestResult {
    let repo = read(&fixture("invalid/layout-component-id"))?;
    let finding = only_finding(&repo)?;
    assert_eq!(
        (finding.rule, finding.severity),
        (Rule::Layout, Severity::Error)
    );
    assert_eq!(finding.path, "components/api/component.toml");
    assert!(finding.message.contains("payments"), "{}", finding.message);
    Ok(())
}

#[test]
fn binary_canon_file_is_a_document_without_text() -> TestResult {
    let repo = read(&valid())?;
    let png = repo
        .documents
        .iter()
        .find(|d| d.path == "canon/logo.png")
        .ok_or("no png document")?;
    assert!(png.text.is_none() && png.front_matter.is_none());
    assert!(repo.read_findings.iter().all(|f| !f.path.contains("canon")));
    Ok(())
}

#[test]
fn finding_paths_use_slashes_and_skip_git_and_bmad() -> TestResult {
    let (_dir, root) = valid_copy()?;
    write(&root, ".git/config", "not = [read")?;
    write(&root, ".git/story-x.md", "---\nbroken: [\n---\n")?;
    write(&root, ".hidden/notes.md", "hidden")?;
    write(&root, "components/app/notes/story-lost.md", "lost")?;
    let story = format!("{CART}/tickets/story-add-item.md");
    let text = fs::read_to_string(root.join(&story))?;
    fs::write(root.join(&story), text.replacen("---\n", "---\nzz: 1\n", 1))?;

    let repo = read(&root)?;
    assert_eq!(repo.read_findings.len(), 2, "{:#?}", repo.read_findings);
    let paths = repo
        .read_findings
        .iter()
        .map(|f| f.path.as_str())
        .chain(repo.documents.iter().map(|d| d.path.as_str()));
    for path in paths {
        assert!(!path.contains('\\'), "{path}");
        assert!(!path.starts_with('/') && !path.contains(':'), "{path}");
        assert!(!path.contains("_bmad") && !path.contains(".git"), "{path}");
        assert!(!path.contains(".hidden"), "{path}");
    }
    assert!(
        repo.read_findings
            .iter()
            .all(|f| !f.message.contains("_bmad"))
    );
    assert!(repo.read_findings.iter().any(|f| f.path == story));
    assert!(
        repo.read_findings
            .iter()
            .any(|f| f.rule == Rule::Layout && f.path == "components/app/notes/story-lost.md")
    );
    Ok(())
}

#[test]
fn a_missing_root_is_a_read_error() -> TestResult {
    let dir = tempfile::tempdir()?;
    let missing = dir.path().join("nope");
    assert!(matches!(read(&missing), Err(ReadError::NotADirectory(p)) if p == missing));
    let file = dir.path().join("file.txt");
    fs::write(&file, "x")?;
    assert!(matches!(read(&file), Err(ReadError::NotADirectory(_))));
    Ok(())
}

// AC: TU-01.2
#[test]
fn a_root_without_loom_toml_is_a_read_error() -> TestResult {
    let dir = tempfile::tempdir()?;
    let err = read(dir.path()).err().ok_or("expected a read error")?;
    assert!(matches!(err, ReadError::NoLoomToml(ref p) if p == dir.path()));
    assert!(err.to_string().contains("has no loom.toml"), "{err}");
    Ok(())
}

#[test]
fn broken_front_matter_fence_is_named() -> TestResult {
    let (_dir, root) = tree(&[(
        "components/app/initiative-x/tickets/story-a.md",
        "---\nid: 1\ntype: story\n# never closed\n",
    )])?;
    let finding = only_finding(&read(&root)?)?.clone();
    assert_eq!(finding.rule, Rule::Parse);
    assert!(finding.message.contains("fence"), "{}", finding.message);
    Ok(())
}

#[test]
fn typed_file_without_front_matter_is_a_parse_finding() -> TestResult {
    let path = "components/app/initiative-x/tickets/story-a.md";
    let (_dir, root) = tree(&[(path, "# A story without front matter\n")])?;
    let finding = only_finding(&read(&root)?)?.clone();
    assert_eq!((finding.rule, finding.path.as_str()), (Rule::Parse, path));
    assert!(
        finding.message.contains("no front matter"),
        "{}",
        finding.message
    );
    Ok(())
}

#[test]
fn two_kinds_of_tables_in_one_breakdown_is_a_layout_finding() -> TestResult {
    let path = "components/app/initiative-x/tickets/tickets.toml";
    let (_dir, root) = tree(&[(
        path,
        "[[entry]]\nid = 1\ntype = \"story\"\ntitle = \"A\"\n\n[[epic]]\nid = 2\nslug = \"epic-b\"\ntitle = \"B\"\n",
    )])?;
    let finding = only_finding(&read(&root)?)?.clone();
    assert_eq!((finding.rule, finding.path.as_str()), (Rule::Layout, path));
    Ok(())
}

#[test]
fn unjoined_files_are_kept() -> TestResult {
    let (_dir, root) = tree(&[
        (
            "components/app/initiative-x/tickets/story-a.md",
            "---\nid: 7\ntype: story\ntitle: \"A\"\n---\n# A\n",
        ),
        (
            "components/app/initiative-x/tickets/story-gone-plan.md",
            "---\nticket: 9\n---\n# Plan\n",
        ),
        (
            "components/app/backlog/bug-b.md",
            "---\ntype: bug\ntitle: \"B\"\n---\n# B\n",
        ),
        (
            "components/app/backlog/bug-b-plan.md",
            "---\nticket: bug-b\nstatus: done\n---\n# Plan\n",
        ),
    ])?;
    let repo = read(&root)?;
    assert!(repo.read_findings.is_empty(), "{:#?}", repo.read_findings);
    let app = component(&repo, "app")?;
    let initiative = app.initiative.as_ref().ok_or("no initiative")?;
    let [leaf] = initiative.stories.as_slice() else {
        return Err("expected the leaf without entry as a story".into());
    };
    assert!(leaf.entry.is_none() && leaf.file.is_some());
    let [plan] = repo.stray_plans.as_slice() else {
        return Err("expected one stray plan".into());
    };
    assert!(plan.path.ends_with("story-gone-plan.md"));
    let bug = app.backlog.first().ok_or("no backlog bug")?;
    assert!(bug.plan.is_some(), "a backlog plan joins by file stem");
    assert!(!bug.is_dropped());
    Ok(())
}

#[test]
fn ticket_files_in_the_earlier_layout_are_layout_findings() -> TestResult {
    let (_dir, root) = tree(&[
        (
            "components/app/initiative-x/story-a.md",
            "---\nid: 1\ntype: story\ntitle: \"A\"\n---\n",
        ),
        (
            "components/app/initiative-x/epic-old/epic-old.md",
            "---\ntype: epic\ntitle: \"Old\"\n---\n",
        ),
        (
            "components/app/initiative-x/epic-old/tickets/tickets.toml",
            "",
        ),
    ])?;
    let repo = read(&root)?;
    let mut paths: Vec<&str> = repo
        .read_findings
        .iter()
        .inspect(|f| assert_eq!(f.rule, Rule::Layout))
        .map(|f| f.path.as_str())
        .collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        [
            "components/app/initiative-x/epic-old",
            "components/app/initiative-x/story-a.md"
        ]
    );
    Ok(())
}

#[test]
fn a_folder_without_component_toml_is_no_component() -> TestResult {
    let (_dir, root) = tree(&[(
        "components/notes/ideas.md",
        "# Ideas
",
    )])?;
    let repo = read(&root)?;
    assert!(repo.read_findings.is_empty(), "{:#?}", repo.read_findings);
    assert!(repo.components.iter().all(|c| c.id != "notes"));
    assert!(
        repo.documents
            .iter()
            .any(|d| d.path == "components/notes/ideas.md")
    );
    Ok(())
}

#[test]
fn ticket_shapes_in_a_folder_without_component_toml_are_layout_findings() -> TestResult {
    let (_dir, root) = tree(&[(
        "components/notes/initiative-x/tickets/epic-a/epic-a.md",
        "---
type: epic
title: \"A\"
---
",
    )])?;
    let repo = read(&root)?;
    let finding = only_finding(&repo)?;
    assert_eq!(
        (finding.rule, finding.path.as_str()),
        (Rule::Layout, "components/notes/initiative-x/tickets/epic-a")
    );
    assert!(repo.components.iter().all(|c| c.id != "notes"));
    Ok(())
}

#[test]
fn needs_in_another_order_is_no_drift() -> TestResult {
    let (_dir, root) = tree(&[
        (
            "components/app/initiative-x/tickets/tickets.toml",
            "[[entry]]
id = 1
type = \"story\"
title = \"A\"
needs = [\"API-1\", \"API-2\"]
",
        ),
        (
            "components/app/initiative-x/tickets/story-a.md",
            "---
id: 1
type: story
title: \"A\"
needs: [API-2, API-1]
---
# A
",
        ),
    ])?;
    let repo = read(&root)?;
    assert!(repo.read_findings.is_empty(), "{:#?}", repo.read_findings);
    let changed = fs::read_to_string(root.join("components/app/initiative-x/tickets/story-a.md"))?
        .replace("[API-2, API-1]", "[API-2]");
    fs::write(
        root.join("components/app/initiative-x/tickets/story-a.md"),
        changed,
    )?;
    let finding = only_finding(&read(&root)?)?.clone();
    assert_eq!(finding.rule, Rule::Drift);
    assert!(finding.message.contains("`needs`"), "{}", finding.message);
    Ok(())
}

// AC: TU-01.2
#[test]
fn a_breakdown_in_backlog_is_a_layout_finding() -> TestResult {
    // `backlog/` has no `tickets/` folder and its leaves have no entries (ticket-tree-rules).
    let repo = read(&fixture("invalid/layout-backlog-breakdown"))?;
    let finding = only_finding(&repo)?;
    assert_eq!(
        (finding.rule, finding.severity),
        (Rule::Layout, Severity::Error)
    );
    assert_eq!(finding.path, "components/app/backlog/tickets.toml");
    Ok(())
}

// AC: TU-01.2
#[test]
fn a_leaf_beside_epics_is_a_layout_finding() -> TestResult {
    // A leaf goes in an initiative's `tickets/` only when the initiative has no epics.
    let repo = read(&fixture("invalid/layout-leaf-beside-epics"))?;
    let finding = only_finding(&repo)?;
    assert_eq!(
        (finding.rule, finding.severity),
        (Rule::Layout, Severity::Error)
    );
    assert_eq!(
        finding.path,
        "components/app/initiative-checkout/tickets/story-gift-note.md"
    );
    Ok(())
}
