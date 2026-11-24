//! Watch planning and in-place registration updates for the dev server.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use notify::Watcher as _;
use notify::event::ModifyKind;
use notify::{Event, EventKind, PathOp, RecommendedWatcher, RecursiveMode, WatchPathConfig};
use tokio::sync::mpsc;

/// Active or desired watcher registrations keyed by watched path.
type Registrations = BTreeMap<PathBuf, RecursiveMode>;

/// Owns the native file watcher and reconciles its registrations as inputs change.
pub(super) struct InputWatcher {
    inputs: Vec<PathBuf>,
    plan: WatchPlan,
    backend: RecommendedWatcher,
    rearm: bool,
}

impl InputWatcher {
    /// Creates a watcher for `inputs` and installs the initial watch registrations.
    pub(super) fn new(
        inputs: &[PathBuf],
        events: mpsc::UnboundedSender<notify::Result<Event>>,
    ) -> Result<Self> {
        let inputs = inputs
            .iter()
            .map(|path| {
                let absolute = std::path::absolute(path)?;
                Ok(canonical_parent(&absolute).unwrap_or(absolute))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let mut backend = notify::recommended_watcher(move |event| _ = events.send(event))
            .context("failed to initialize file watcher")?;
        let plan = WatchPlan::new(&inputs);
        update_watches(
            &mut backend,
            &Registrations::new(),
            &plan.registrations,
            false,
        )?;
        Ok(Self {
            inputs,
            plan,
            backend,
            rearm: false,
        })
    }

    /// Returns `None` to ignore `event`, `Some(false)` for a change, or `Some(true)` to rearm.
    pub(super) fn change(&self, event: &Event) -> Option<bool> {
        self.plan.change(event)
    }

    /// Recomputes the watch plan and updates registrations in place, with one bounded recheck.
    pub(super) fn refresh(&mut self, invalidated: bool) {
        self.rearm |= invalidated;
        // A new link or missing component can change before its parent watch starts.
        for _ in 0..2 {
            let plan = WatchPlan::new(&self.inputs);
            if !self.rearm && self.plan == plan {
                return;
            }

            let active = match active_watches(&self.backend) {
                Ok(active) => active,
                Err(error) => {
                    self.rearm = true;
                    tracing::warn!(
                        error = %format_args!("{error:#}"),
                        "failed to list file watches",
                    );
                    return;
                }
            };
            if let Err(error) =
                update_watches(&mut self.backend, &active, &plan.registrations, self.rearm)
            {
                self.rearm = true;
                tracing::warn!(
                    error = %format_args!("{error:#}"),
                    "failed to refresh file watches",
                );
                if WatchPlan::new(&self.inputs) != plan {
                    continue;
                }
                return;
            }
            self.plan = plan;
            self.rearm = false;
        }
    }
}

/// Reads the backend's current watch registrations into a deduplicated map.
fn active_watches(backend: &RecommendedWatcher) -> Result<Registrations> {
    let mut active = Registrations::new();
    for watch in backend
        .watched_paths()
        .context("failed to list file watches")?
    {
        ensure!(
            active.insert(watch.path, watch.recursive_mode).is_none(),
            "file watcher reported duplicate registrations"
        );
    }
    Ok(active)
}

/// Applies removed watches in reverse order, then added, mode-changed, or rearmed watches.
fn update_watches(
    backend: &mut RecommendedWatcher,
    active: &Registrations,
    desired: &Registrations,
    rearm: bool,
) -> Result<()> {
    let operations: Vec<_> = active
        .keys()
        .rev()
        .filter(|path| !desired.contains_key(*path))
        .map(|path| PathOp::Unwatch(path.clone()))
        .chain(
            desired
                .iter()
                .filter(|(path, mode)| rearm || active.get(*path) != Some(*mode))
                .map(|(path, &mode)| PathOp::Watch(path.clone(), WatchPathConfig::new(mode))),
        )
        .collect();
    if operations.is_empty() {
        return Ok(());
    }

    backend.update_paths(operations).map_err(|error| {
        let context = match error.origin {
            Some(op @ PathOp::Watch(..)) => format!("failed to watch {}", op.as_path().display()),
            Some(op @ PathOp::Unwatch(..)) => {
                format!("failed to remove watch for {}", op.as_path().display())
            }
            Some(op) => format!("failed to update watch for {}", op.as_path().display()),
            None => "failed to update file watches".to_owned(),
        };
        anyhow::Error::new(error.source).context(context)
    })
}

/// Resolved input paths and the directory registrations required to observe them.
#[derive(Clone, Default, PartialEq, Eq)]
struct WatchPlan {
    inputs: BTreeSet<PathBuf>,
    registrations: Registrations,
}

impl WatchPlan {
    /// Builds a watch plan for all inputs, logging resolution errors and keeping partial tracking.
    fn new(inputs: &[PathBuf]) -> Self {
        let mut plan = Self::default();
        for input in inputs {
            let mut entry = Self::default();
            let result = entry.add_input(input).or_else(|error| {
                // A parent can disappear before its child is recorded.
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) {
                    entry = Self::default();
                    entry.add_input(input)
                } else {
                    Err(error)
                }
            });
            if let Err(error) = result {
                tracing::warn!(
                    path = %input.display(),
                    %error,
                    "failed to resolve watch target",
                );
            }
            plan.extend(entry);
        }
        plan
    }

    /// Resolves one input, following symlinks from the input entry onward without watching `/`.
    fn add_input(&mut self, input: &Path) -> io::Result<()> {
        let mut pending = canonical_parent(input).unwrap_or_else(|_| input.to_owned());
        'resolve: for _ in 0..=40 {
            let mut path = PathBuf::new();
            let mut components = pending.components();
            while let Some(component) = components.next() {
                match component {
                    Component::RootDir => path.push(component),
                    Component::ParentDir => _ = path.pop(),
                    Component::Normal(name) => {
                        path.push(name);
                        match fs::symlink_metadata(&path) {
                            Ok(metadata) if metadata.is_symlink() => {
                                let link = match path.read_link() {
                                    Ok(link) => link,
                                    Err(error)
                                        if matches!(
                                            error.kind(),
                                            io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
                                        ) =>
                                    {
                                        continue 'resolve;
                                    }
                                    Err(error) => return Err(error),
                                };
                                if !is_root_child(&path) {
                                    self.track(&path)?;
                                }
                                pending = path.with_file_name(link).join(components.as_path());
                                continue 'resolve;
                            }
                            Ok(_) => {}
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                                self.track(&path)?;
                                return Ok(());
                            }
                            Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                                self.track(path.parent().unwrap_or(&path))?;
                                return Ok(());
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    _ => {}
                }
            }
            if path.parent().is_some() {
                let path = self.track(&path)?;
                if path.is_dir() {
                    self.register(path, RecursiveMode::Recursive);
                }
            }
            return Ok(());
        }
        Err(io::Error::other("too many symlinks"))
    }

    /// Records a tracked path and registers a non-recursive watch on its canonical parent.
    fn track(&mut self, path: &Path) -> io::Result<PathBuf> {
        let path = canonical_parent(path)?;
        if let Some(parent) = path.parent().filter(|parent| parent.parent().is_some()) {
            self.register(parent.to_owned(), RecursiveMode::NonRecursive);
        }
        self.inputs.insert(path.clone());
        Ok(path)
    }

    /// Inserts a watch registration, giving recursive mode precedence over non-recursive mode.
    fn register(&mut self, path: PathBuf, mode: RecursiveMode) {
        self.registrations
            .entry(path)
            .and_modify(|existing| {
                if mode == RecursiveMode::Recursive {
                    *existing = RecursiveMode::Recursive;
                }
            })
            .or_insert(mode);
    }

    /// Merges another plan into `self`, preserving recursive mode precedence.
    fn extend(&mut self, other: Self) {
        self.inputs.extend(other.inputs);
        for (path, mode) in other.registrations {
            self.register(path, mode);
        }
    }

    /// Returns `None` to ignore `event`, `Some(false)` for a change, or `Some(true)` to rearm.
    fn change(&self, event: &Event) -> Option<bool> {
        if event.need_rescan() {
            return Some(true);
        }
        let invalidated = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_)) | EventKind::Remove(_)
        ) && event
            .paths
            .iter()
            .any(|path| self.registrations.contains_key(path));
        if invalidated || is_input_change(event, &self.inputs) {
            Some(invalidated)
        } else {
            None
        }
    }
}

/// Canonicalizes an input or link destination's parent while preserving its final component.
fn canonical_parent(path: &Path) -> io::Result<PathBuf> {
    match path.parent().zip(path.file_name()) {
        Some((parent, name)) => Ok(parent.canonicalize()?.join(name)),
        None => Ok(path.to_owned()),
    }
}

/// Reports whether `path` is a direct child of the filesystem root.
fn is_root_child(path: &Path) -> bool {
    path.parent()
        .is_some_and(|parent| parent.parent().is_none())
}

/// Reports whether an event modifies any tracked input or descendant, ignoring sibling events.
fn is_input_change(event: &Event, inputs: &BTreeSet<PathBuf>) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event
        .paths
        .iter()
        .any(|path| inputs.iter().any(|input| path.starts_with(input)))
}

#[cfg(test)]
pub(super) mod tests {
    use notify::event::{CreateKind, Flag, RenameMode};

    use super::*;

    // ── InputWatcher::new ──

    #[test]
    fn keeps_directory_watch_recursive_when_a_file_shares_it() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let assets = root.join("assets");
        fs::create_dir(&assets).unwrap();
        let config = assets.join("site.toml");
        fs::write(&config, "").unwrap();

        for inputs in [
            [assets.clone(), config.clone()],
            [config.clone(), assets.clone()],
        ] {
            let (events, _receiver) = mpsc::unbounded_channel();
            let watcher = InputWatcher::new(&inputs, events).unwrap();

            assert_eq!(
                active_watches(&watcher.backend).unwrap(),
                Registrations::from([
                    (root.clone(), RecursiveMode::NonRecursive),
                    (assets.clone(), RecursiveMode::Recursive),
                ])
            );
        }
    }

    #[test]
    fn registers_a_distinct_parent_below_a_recursive_input() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let parent = root.join("content/external");
        fs::create_dir_all(&parent).unwrap();
        let file = parent.join("site.toml");
        fs::write(&file, "").unwrap();
        let (events, _receiver) = mpsc::unbounded_channel();

        let watcher = InputWatcher::new(&[root.join("content"), file], events).unwrap();

        assert_eq!(
            active_watches(&watcher.backend).unwrap(),
            Registrations::from([
                (root.clone(), RecursiveMode::NonRecursive),
                (root.join("content"), RecursiveMode::Recursive),
                (parent, RecursiveMode::NonRecursive),
            ])
        );
    }

    #[test]
    fn does_not_watch_root_for_input_under_symlinked_parent() {
        let temp = tempfile::tempdir().unwrap();
        let real_parent = temp.path().join("real");
        let site = real_parent.join("site");
        fs::create_dir_all(site.join("content")).unwrap();
        let external = temp.path().join("external");
        fs::create_dir(&external).unwrap();
        let external_config = external.join("site.toml");
        fs::write(&external_config, "").unwrap();
        // Keep `temp` non-canonical to exercise root-level symlinks on macOS.
        std::os::unix::fs::symlink(&external_config, site.join("config.toml")).unwrap();
        let linked_parent = temp.path().join("linked");
        std::os::unix::fs::symlink(&real_parent, &linked_parent).unwrap();
        let linked_site = linked_parent.join("site");
        let (events, _receiver) = mpsc::unbounded_channel();

        let watcher = InputWatcher::new(
            &[linked_site.join("config.toml"), linked_site.join("content")],
            events,
        )
        .unwrap();

        let active = active_watches(&watcher.backend).unwrap();
        assert!(!active.contains_key(Path::new("/")));
        assert!(!active.contains_key(&temp.path().canonicalize().unwrap()));
        let canonical_site = site.canonicalize().unwrap();
        assert_eq!(
            active,
            Registrations::from([
                (canonical_site.clone(), RecursiveMode::NonRecursive),
                (canonical_site.join("content"), RecursiveMode::Recursive),
                (
                    external.canonicalize().unwrap(),
                    RecursiveMode::NonRecursive
                ),
            ])
        );
    }

    // ── InputWatcher::refresh ──

    #[test]
    fn replaces_external_target_parent_without_retaining_old_registration() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        for dir in ["site", "chain", "first", "second"] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        let config = root.join("site/config.toml");
        let link = root.join("chain/next.toml");
        let first = root.join("first/site.toml");
        let second = root.join("second/site.toml");
        fs::write(&first, "").unwrap();
        fs::write(&second, "").unwrap();
        replace_symlink(&config, &link);
        replace_symlink(&link, &first);
        let (events, _receiver) = mpsc::unbounded_channel();
        let mut watcher = InputWatcher::new(&[config], events).unwrap();

        let expected = |target_parent| -> Registrations {
            ["site", "chain", target_parent]
                .map(|dir| (root.join(dir), RecursiveMode::NonRecursive))
                .into()
        };
        assert_eq!(active_watches(&watcher.backend).unwrap(), expected("first"));
        replace_symlink(&link, &second);
        watcher.refresh(false);
        fs::remove_file(first).unwrap();

        assert_eq!(
            active_watches(&watcher.backend).unwrap(),
            expected("second")
        );
    }

    #[test]
    fn advances_through_missing_components_and_tracks_directory_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("site")).unwrap();
        fs::create_dir(root.join("external")).unwrap();
        let config = root.join("site/config.toml");
        let current = root.join("external/current");
        let new = root.join("external/new");
        let nested = new.join("nested");
        let target = nested.join("site.toml");
        replace_symlink(&config, &current.join("nested/site.toml"));
        replace_symlink(&current, Path::new("new"));
        let (events, _receiver) = mpsc::unbounded_channel();
        let mut watcher = InputWatcher::new(&[config], events).unwrap();

        for (component, parent) in [(&new, &new), (&nested, &nested), (&target, &nested)] {
            if component == &target {
                fs::write(component, "").unwrap();
            } else {
                fs::create_dir(component).unwrap();
            }
            let event = Event::new(EventKind::Create(CreateKind::Any)).add_path(component.clone());
            assert!(watcher.plan.change(&event).is_some(), "{component:?}");
            watcher.refresh(false);
            let expected = [root.join("site"), root.join("external"), parent.clone()]
                .map(|path| (path, RecursiveMode::NonRecursive));
            assert_eq!(
                active_watches(&watcher.backend).unwrap(),
                Registrations::from(expected)
            );
        }
        assert!(watcher.plan.inputs.contains(&current));
    }

    // ── update_watches ──

    #[test]
    fn updates_registrations_and_reconciles_from_active_watches_after_error() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let (events, _receiver) = mpsc::unbounded_channel();
        let mut backend = notify::recommended_watcher(move |event| _ = events.send(event)).unwrap();

        let initial = Registrations::from([(first.clone(), RecursiveMode::NonRecursive)]);
        update_watches(&mut backend, &Registrations::new(), &initial, false).unwrap();
        assert_eq!(active_watches(&backend).unwrap(), initial);

        let broken = Registrations::from([
            (first.clone(), RecursiveMode::Recursive),
            (root.join("missing"), RecursiveMode::NonRecursive),
        ]);
        assert!(update_watches(&mut backend, &initial, &broken, false).is_err());

        let recovered = Registrations::from([(second, RecursiveMode::Recursive)]);
        let active = active_watches(&backend).unwrap();
        update_watches(&mut backend, &active, &recovered, true).unwrap();
        assert_eq!(active_watches(&backend).unwrap(), recovered);
    }

    // ── WatchPlan::new ──

    #[test]
    fn keeps_parent_watch_for_missing_symlink_destination() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let config = root.join("config.toml");
        let link = root.join("current.toml");
        let target_parent = root.join("config");
        let target = target_parent.join("site.toml");
        fs::create_dir(&target_parent).unwrap();
        fs::write(&target, "").unwrap();
        std::os::unix::fs::symlink("current.toml", &config).unwrap();
        std::os::unix::fs::symlink("config/site.toml", &link).unwrap();
        let inputs = [config];
        let original = WatchPlan::new(&inputs);

        fs::remove_file(&target).unwrap();
        let missing = WatchPlan::new(&inputs);

        assert_eq!(original.inputs, missing.inputs);
        assert_eq!(original.registrations, missing.registrations);
    }

    #[test]
    fn handles_not_a_directory_symlink_target_without_dropping_other_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let blocker = root.join("blocker");
        let content = root.join("content");
        let config = root.join("config.toml");
        fs::write(&blocker, "").unwrap();
        fs::create_dir(&content).unwrap();
        std::os::unix::fs::symlink(blocker.join("nested/site.toml"), &config).unwrap();

        let plan = WatchPlan::new(&[config.clone(), content.clone()]);

        assert!(plan.inputs.contains(&config));
        assert!(plan.inputs.contains(&blocker));
        assert!(plan.inputs.contains(&content));
        assert_eq!(
            plan.registrations,
            Registrations::from([
                (root, RecursiveMode::NonRecursive),
                (content, RecursiveMode::Recursive),
            ])
        );
    }

    #[test]
    fn keeps_healthy_inputs_when_another_input_loops() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let content = root.join("content");
        let config = root.join("config.toml");
        fs::create_dir(&content).unwrap();
        std::os::unix::fs::symlink(&config, &config).unwrap();

        let plan = WatchPlan::new(&[config.clone(), content.clone()]);

        assert!(plan.inputs.contains(&config));
        assert!(plan.inputs.contains(&content));
        assert_eq!(
            plan.registrations,
            Registrations::from([
                (root, RecursiveMode::NonRecursive),
                (content, RecursiveMode::Recursive),
            ])
        );
    }

    #[test]
    fn skips_root_watch_for_dangling_root_child_target() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let config = root.join("config.toml");
        std::os::unix::fs::symlink("/kiln-missing-root-child/site.toml", &config).unwrap();

        let plan = WatchPlan::new(std::slice::from_ref(&config));

        assert!(plan.inputs.contains(&config));
        assert!(!plan.registrations.contains_key(Path::new("/")));
        assert_eq!(
            plan.registrations,
            Registrations::from([(root, RecursiveMode::NonRecursive)])
        );
    }

    #[test]
    fn skips_root_watch_for_symlink_to_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let config = root.join("config.toml");
        std::os::unix::fs::symlink("/", &config).unwrap();

        let plan = WatchPlan::new(std::slice::from_ref(&config));

        assert!(plan.inputs.contains(&config));
        assert!(!plan.inputs.contains(Path::new("/")));
        assert!(!plan.registrations.contains_key(Path::new("/")));
        assert_eq!(
            plan.registrations,
            Registrations::from([(root, RecursiveMode::NonRecursive)])
        );
    }

    // ── WatchPlan::change ──

    #[test]
    fn matches_inputs_and_their_descendants() {
        let root = Path::new("/site");
        let plan = WatchPlan {
            inputs: BTreeSet::from([root.join("config.toml"), root.join("content")]),
            registrations: Registrations::from([
                (root.to_owned(), RecursiveMode::NonRecursive),
                (root.join("content"), RecursiveMode::Recursive),
            ]),
        };
        let modify = EventKind::Modify(ModifyKind::Any);
        for (kind, paths, expected) in [
            (modify, &["config.toml"][..], Some(false)),
            (modify, &["content/posts/hello/index.md"], Some(false)),
            (
                EventKind::Create(CreateKind::File),
                &["content/new.md"],
                Some(false),
            ),
            (
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &["config.toml.tmp", "config.toml"],
                Some(false),
            ),
            (
                EventKind::Create(CreateKind::Folder),
                &["content"],
                Some(true),
            ),
            (modify, &["config.toml.swp"], None),
            (modify, &["public/index.html"], None),
            (
                EventKind::Create(CreateKind::Folder),
                &[".kiln-build-abc123/output"],
                None,
            ),
            (
                EventKind::Access(notify::event::AccessKind::Any),
                &["config.toml"],
                None,
            ),
            (EventKind::Other, &[], None),
            (EventKind::Other, &["config.toml"], None),
            (modify, &[], None),
            (modify, &["../outside.md"], None),
        ] {
            let event = paths.iter().fold(Event::new(kind), |event, path| {
                event.add_path(root.join(path))
            });

            assert_eq!(plan.change(&event), expected, "{event:?}");
        }
    }

    #[test]
    fn accepts_rescan_regardless_of_paths() {
        let plan = WatchPlan {
            inputs: BTreeSet::from([PathBuf::from("/site/content")]),
            registrations: Registrations::new(),
        };
        for paths in [&[][..], &["/site/public/output.html"], &["/outside.md"]] {
            let event = paths.iter().fold(
                Event::new(EventKind::Other).set_flag(Flag::Rescan),
                |event, path| event.add_path(PathBuf::from(path)),
            );

            assert_eq!(plan.change(&event), Some(true), "{event:?}");
        }
    }

    pub(in crate::serve) fn replace_symlink(link: &Path, target: &Path) {
        let staged = link.with_extension("tmp");
        std::os::unix::fs::symlink(target, &staged).unwrap();
        fs::rename(staged, link).unwrap();
    }
}
