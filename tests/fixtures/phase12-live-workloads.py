#!/usr/bin/env python3
"""Create deterministic, local Git workloads for Phase 12 live trials.

Example: python3 tests/fixtures/phase12-live-workloads.py --output /tmp/phase12-seeds
The output contains seed repositories and manifest.json. It starts no models.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess


# Three repositories are deliberately interleaved so each prefix uses all three.
INDEPENDENT = [
    ("manifest", "manifest_kit", "validate_project", "Return sorted missing required keys from a mapping; required keys are name, version, modules.", "{'name':'widget','version':'1.2','modules':['core']}", "[]", "self.assertEqual(validate_project({'name':'w','version':'1','modules':[]}), [])\n        self.assertEqual(validate_project({'name':'w'}), ['modules', 'version'])"),
    ("changelog", "change_kit", "parse_heading", "Parse ## version - YYYY-MM-DD headings; reject malformed dates and versions with ValueError.", "'## 1.2.0 - 2026-09-01'", "('1.2.0', '2026-09-01')", "self.assertRaises(ValueError, parse_heading, '## nope - 2026-99-99')"),
    ("docs", "doccheck", "normalize_anchor", "Convert a heading to a lowercase GitHub-style anchor; strip punctuation and collapse separators.", "'Install, Build & Test!'", "'install-build-test'", "self.assertEqual(normalize_anchor(' A  B! '), 'a-b')"),
    ("manifest", "manifest_kit", "parse_version", "Parse exactly three non-negative integer components from a dotted version. Reject malformed strings with ValueError.", "'2.4.1'", "(2, 4, 1)", "for value in ('1.2', '1.2.x', '-1.2.3', '1.2.3.4'):\n            self.assertRaises(ValueError, parse_version, value)"),
    ("changelog", "change_kit", "entries_for_type", "Select strings whose prefix is exactly requested type followed by colon; preserve source order.", "['fix: crash','feat: api','fix: leak'], 'fix'", "['fix: crash', 'fix: leak']", "self.assertEqual(entries_for_type(['fixable: no','fix: yes'], 'fix'), ['fix: yes'])"),
    ("docs", "doccheck", "local_links", "Extract local Markdown link targets, excluding URL, mailto, and fragment-only targets; preserve order.", "'[a](guide.md) [b](https://x.test) [c](#top)'", "['guide.md']", "self.assertEqual(local_links('[x](mailto:a@b.test) [y](../a.md)'), ['../a.md'])"),
    ("manifest", "manifest_kit", "normalize_modules", "Return unique module names sorted lexicographically; reject empty or whitespace-padded names.", "['zeta','alpha','zeta']", "['alpha', 'zeta']", "self.assertRaises(ValueError, normalize_modules, ['ok', ' padded'])\n        self.assertRaises(ValueError, normalize_modules, [''])"),
    ("changelog", "change_kit", "render_section", "Render a version heading then '- ' bullets and final newline; empty entries render heading plus newline.", "'1.2.0', ['Added API']", "'## 1.2.0\\n- Added API\\n'", "self.assertEqual(render_section('1.0', []), '## 1.0\\n')"),
    ("docs", "doccheck", "broken_targets", "Return referenced local targets that do not exist relative to the Markdown file, ignoring anchors.", "'README.md', '[ok](guide.md) [bad](missing.md#x)'", "['missing.md']", "self.assertEqual(broken_targets('README.md', '[x](https://x.test)'), [])"),
    ("manifest", "manifest_kit", "topological_order", "Return deterministic dependency-first order; reject cycles and unknown names with ValueError.", "{'app':['lib'], 'lib':[]}", "['lib', 'app']", "self.assertRaises(ValueError, topological_order, {'a':['b'],'b':['a']})\n        self.assertRaises(ValueError, topological_order, {'a':['missing']})"),
    ("changelog", "change_kit", "latest_version", "Return first valid version heading in text, ignoring prose and malformed headings; None if absent.", "'Notes\\n## nope\\n## 3.0.0 - 2026-01-01'", "'3.0.0'", "self.assertIsNone(latest_version('Notes\\n## nope'))"),
    ("docs", "doccheck", "heading_ids", "Return normalized anchors for ATX headings in source order; ignore non-heading lines.", "'# Intro\\ntext\\n## Build & test'", "['intro', 'build-test']", "self.assertEqual(heading_ids('text\\n#### Four'), ['four'])"),
    ("manifest", "manifest_kit", "safe_relative_path", "Accept normalized relative POSIX paths without empty, dot, or .. components or leading slash.", "'src/pkg.py'", "True", "for value in ('../secret', '/absolute', 'a/../b', 'a//b', ''):\n            self.assertFalse(safe_relative_path(value))"),
    ("changelog", "change_kit", "compare_versions", "Compare dotted numeric versions of equal component count; return -1, 0, or 1; reject malformed versions.", "'1.10.0', '1.9.9'", "1", "self.assertEqual(compare_versions('2.0', '2.0'), 0)\n        self.assertRaises(ValueError, compare_versions, '1.x', '1.0')"),
    ("docs", "doccheck", "is_external_link", "Return True for http, https and mailto schemes case-insensitively; False otherwise.", "'HTTPS://example.test'", "True", "self.assertFalse(is_external_link('/guide.md'))"),
    ("manifest", "manifest_kit", "validate_file_list", "Return sorted duplicate path strings; reject unsafe paths with ValueError.", "['src/a.py','README.md','src/a.py']", "['src/a.py']", "self.assertRaises(ValueError, validate_file_list, ['../escape'])"),
    ("changelog", "change_kit", "breaking_entries", "Return entries containing exact marker BREAKING: in original order.", "['feat: x','BREAKING: remove y','fix: z']", "['BREAKING: remove y']", "self.assertEqual(breaking_entries(['not BREAKING: yes']), ['not BREAKING: yes'])"),
    ("docs", "doccheck", "strip_code_fences", "Remove fenced code blocks and delimiters, retaining surrounding text and line breaks.", "'before\\n```py\\nx=1\\n```\\nafter\\n'", "'before\\n\\nafter\\n'", "self.assertEqual(strip_code_fences('plain\\n'), 'plain\\n')"),
    ("manifest", "manifest_kit", "release_tag", "Format package mapping name/version as name-vversion; reject missing or empty values.", "{'name':'widget','version':'1.2.0'}", "'widget-v1.2.0'", "self.assertRaises(ValueError, release_tag, {'name':'','version':'1'})"),
    ("changelog", "change_kit", "group_entries", "Group typed entries by text before first colon; preserve within-group order, sorted keys; skip untyped entries.", "['fix: a','feat: b','fix: c']", "{'feat': ['feat: b'], 'fix': ['fix: a', 'fix: c']}", "self.assertEqual(group_entries(['loose']), {})"),
]


def task_id(i: int) -> str:
    return f"task-{i:02d}"


def scaffold(root: Path, rows: list[tuple], dependencies: dict[int, list[int]], integrations: dict[int, tuple] | None = None) -> list[dict]:
    tasks = []
    for i, row in enumerate(rows, 1):
        repo, package, func, spec, args, expected, extra = row
        repo_dir = root / repo
        (repo_dir / package).mkdir(parents=True, exist_ok=True)
        tests = repo_dir / "evaluator"
        tests.mkdir(exist_ok=True)
        (repo_dir / package / "__init__.py").write_text("")
        module = f"task_{i:02d}"
        (repo_dir / package / f"{module}.py").write_text(f"def {func}(*args):\n    raise NotImplementedError('implement {func}')\n")
        (tests / "__init__.py").write_text("")
        checks = f"self.assertEqual({func}({args}), {expected})\n        {extra}"
        integration = integrations.get(i) if integrations else None
        if integration:
            spec, imports, checks = integration
        body_lines = checks.splitlines()
        body = "\n".join(("            " if line.startswith("            ") else "        ") + line.strip() for line in body_lines)
        test = f"import hashlib\nimport json\nimport os\nfrom pathlib import Path\nimport tempfile\nimport unittest\nfrom {package}.{module} import {func}\n{imports if integration else ''}\nclass Contract(unittest.TestCase):\n    def setUp(self):\n        self._old_cwd = os.getcwd()\n        self._tmp = tempfile.TemporaryDirectory()\n        os.chdir(self._tmp.name)\n        Path('guide.md').write_text('guide\\n')\n        Path('manifest.json').write_text('{{\"name\":\"widget\",\"version\":\"1.0\",\"packages\":[\"core\"]}}\\n')\n        Path('out').mkdir()\n        Path('out/release.json').write_text(json.dumps({{'filename': 'widget-1.0.json'}}))\n\n    def tearDown(self):\n        os.chdir(self._old_cwd)\n        self._tmp.cleanup()\n\n    def test_behavior(self):\n{body}\n\nif __name__ == '__main__': unittest.main()\n"
        (repo_dir / f"evaluator/test_{module}.py").write_text(test)
        deps = dependencies.get(i, [])
        if deps:
            spec += " Predecessor artifacts are incorporated into this task's frozen base revision before its isolated Git worktree is spawned. Consume those committed APIs directly. Each task runs in its own worktree; never assume later sibling edits appear in this checkout."
        task = {"task_id": task_id(i), "repository": repo, "repository_identity": f"phase12-{repo}-v1", "specification": spec,
                "allowed_paths": [f"{package}/{module}.py", f"tests/test_worker_{module}.py"], "dependencies": [task_id(x) for x in deps],
                "check": f"python3 -m unittest evaluator.test_{module} -v", "golden_test": f"evaluator/test_{module}.py", "seed_module": f"{package}/{module}.py", "integration": bool(integration)}
        tasks.append(task)
    return tasks


def git(repo: Path, *args: str, env: dict[str, str]) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], env=env, text=True).strip()


def init_repo(path: Path, name: str, env: dict[str, str]) -> str:
    path.mkdir(parents=True, exist_ok=True)
    git(path, "init", "-q", "-b", "main", env=env)
    git(path, "config", "user.name", "Phase 12 Fixture", env=env)
    git(path, "config", "user.email", "phase12@example.invalid", env=env)
    git(path, "add", ".", env=env)
    fixed = {**env, "GIT_AUTHOR_DATE": "2001-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2001-01-01T00:00:00Z"}
    git(path, "commit", "-q", "-m", f"{name} seed", env=fixed)
    return git(path, "rev-parse", "HEAD", env=env)


def generate(output: Path) -> dict:
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"output must be empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull, "LC_ALL": "C"})
    independent_root = output / "independent"
    rows = scaffold(independent_root, INDEPENDENT, {})
    independent_shas = {repo: init_repo(independent_root / repo, repo, env) for repo in sorted({r[0] for r in INDEPENDENT})}

    # Release bundle pipeline: each step adds a distinct capability. The DAG has
    # branches, joins, and real API-consuming milestones rather than a serial chain.
    bundle_rows = [
        ("release-bundle", "bundle", "read_manifest", "Read JSON manifest path and return the complete decoded mapping.", "'manifest.json'", "{'name': 'widget', 'version': '1.0', 'packages': ['core']}", "self.assertEqual(sorted(read_manifest('manifest.json')), ['name', 'packages', 'version'])"),
        ("release-bundle", "bundle", "select_changes", "Select change titles for chosen packages in source order.", "[{'package':'core','title':'Fix'},{'package':'docs','title':'Guide'}], ['core']", "['Fix']", "self.assertEqual(select_changes([], ['core']), [])"),
        ("release-bundle", "bundle", "render_notes", "Render titles as newline-terminated bullets; empty lists return empty text.", "['Fix','Guide']", "'- Fix\\n- Guide\\n'", "self.assertEqual(render_notes([]), '')"),
        ("release-bundle", "bundle", "bundle_filename", "Format safe name and version as <name>-<version>.json; reject slash, backslash, and empty parts.", "'widget', '1.0'", "'widget-1.0.json'", "for value in ('', '../x', 'a\\\\b'):\n            self.assertRaises(ValueError, bundle_filename, value, '1')"),
        ("release-bundle", "bundle", "build_bundle", "Implement build_bundle(manifest_path, changes): call read_manifest, select_changes, render_notes and bundle_filename from predecessor modules and return {'filename','notes'}.", "'manifest.json', [{'package':'core','title':'Fix'}]", "{'filename': 'widget-1.0.json', 'notes': '- Fix\\n'}", "self.assertEqual(build_bundle('manifest.json', []), {'filename':'widget-1.0.json','notes':''})"),
        ("release-bundle", "bundle", "validate_manifest", "Validate manifest mapping: require nonempty name/version and nonempty unique string package names; raise ValueError otherwise.", "{'name':'widget','version':'1.0','packages':['core']} ", "True", "self.assertRaises(ValueError, validate_manifest, {'name':'w','version':'1','packages':['core','core']})"),
        ("release-bundle", "bundle", "normalize_changes", "Normalize change records by trimming title and package; reject missing or blank values.", "[{'package':' core ','title':' Fix '} ]", "[{'package': 'core', 'title': 'Fix'}]", "self.assertRaises(ValueError, normalize_changes, [{'package':'core','title':' '}])"),
        ("release-bundle", "bundle", "render_json", "Serialize a mapping as deterministic indented JSON with final newline and sorted keys.", "{'z':1,'a':2}", "'{\\n  \"a\": 2,\\n  \"z\": 1\\n}\\n'", "self.assertTrue(render_json({}).endswith('\\n'))"),
        ("release-bundle", "bundle", "write_bundle", "Write build_bundle output using predecessor render_json as deterministic JSON to a destination; create parent directories and return destination path string.", "'manifest.json', [], 'out/release.json'", "'out/release.json'", "self.assertTrue(write_bundle('manifest.json', [], 'out/release.json').endswith('release.json'))"),
        ("release-bundle", "bundle", "validate_bundle_result", "Implement validate_bundle_result(bundle): require filename ending .json and notes string ending newline unless empty; raise ValueError otherwise. Consume validate_manifest and build_bundle APIs in integration.", "{'filename':'widget-1.0.json','notes':'- Fix\\n'}", "True", "self.assertRaises(ValueError, validate_bundle_result, {'filename':'x.txt','notes':''})"),
        ("release-bundle", "bundle", "changes_by_package", "Group normalized change records by package; preserve order within each package and sort package keys.", "[{'package':'z','title':'Z'},{'package':'a','title':'A'}]", "{'a': [{'package': 'a', 'title': 'A'}], 'z': [{'package': 'z', 'title': 'Z'}]}", "self.assertEqual(changes_by_package([]), {})"),
        ("release-bundle", "bundle", "breaking_changes", "Return normalized change records whose title begins with BREAKING: in source order.", "[{'package':'core','title':'BREAKING: API'},{'package':'docs','title':'Guide'}]", "[{'package': 'core', 'title': 'BREAKING: API'}]", "self.assertEqual(breaking_changes([]), [])"),
        ("release-bundle", "bundle", "package_summary", "Summarize grouped changes as sorted package names and total count.", "{'z':[1,2],'a':[3]}", "{'packages': ['a', 'z'], 'count': 3}", "self.assertEqual(package_summary({}), {'packages': [], 'count': 0})"),
        ("release-bundle", "bundle", "release_plan", "Build deterministic dependency-first package order using manifest dependency mapping; reject cycles and unknown dependencies.", "{'app':['core'],'core':[]}", "['core', 'app']", "self.assertRaises(ValueError, release_plan, {'a':['b'],'b':['a']})"),
        ("release-bundle", "bundle", "verify_outputs", "Verify bundle mapping filename exists beneath output root and JSON content matches mapping; reject path escape or mismatch.", "'out', 'release.json', {'filename':'widget-1.0.json'}", "True", "self.assertRaises(ValueError, verify_outputs, 'out', '../escape', {})"),
        ("release-bundle", "bundle", "release_digest", "Return lowercase SHA-256 hex digest of UTF-8 notes text.", "'release notes\\n'", "hashlib.sha256('release notes\\n'.encode()).hexdigest()", "self.assertEqual(len(release_digest('')), 64)"),
        ("release-bundle", "bundle", "render_release_index", "Render package summary and digest as deterministic JSON index with final newline.", "['core'], 'abc123'", "'{\\n  \"digest\": \"abc123\",\\n  \"packages\": [\\n    \"core\"\\n  ]\\n}\\n'", "self.assertTrue(render_release_index([], 'x').endswith('\\n'))"),
        ("release-bundle", "bundle", "clean_output", "Remove only generated files listed in a release index beneath the output root; reject paths escaping root.", "'out', ['release.json']", "['release.json']", "self.assertRaises(ValueError, clean_output, 'out', ['../keep.txt'])"),
        ("release-bundle", "bundle", "assemble_release", "Integrate build_bundle, validate_bundle_result, release_plan and release_digest from predecessor modules; return filename, dependency order, notes digest and valid=True. Inputs are manifest path, changes and dependency mapping.", "'manifest.json', [{'package':'core','title':'Fix'}], {'core':[]}", "{'filename':'widget-1.0.json','order':['core'],'digest':hashlib.sha256('- Fix\\n'.encode()).hexdigest(),'valid':True}", "self.assertTrue(assemble_release('manifest.json', [], {'core':[]})['valid'])"),
        ("release-bundle", "bundle", "publish_report", "Create final release report by consuming assemble_release and package_summary predecessor APIs; require validation flag and return sorted package list, order and digest.", "{'filename':'widget-1.0.json','order':['core'],'digest':'abc','valid':True}, {'packages':['core'],'count':1}", "{'filename':'widget-1.0.json','packages':['core'],'order':['core'],'digest':'abc'}", "assembled = assemble_release('manifest.json', [{'package':'core','title':'Fix'}], {'core':[]})\n        summary = package_summary({'core': [{'package':'core','title':'Fix'}]})\n        self.assertEqual(publish_report(assembled, summary), {'filename':'widget-1.0.json','packages':['core'],'order':['core'],'digest':hashlib.sha256('- Fix\\n'.encode()).hexdigest()})\n        self.assertRaises(ValueError, publish_report, {'valid':False}, {})"),
    ]
    deps = {5:[1,2,3,4], 6:[1], 7:[2], 8:[3], 9:[5,6,7,8], 10:[5,6,9], 11:[7], 12:[7], 13:[11,12], 14:[1,13], 15:[8], 16:[3,13], 17:[13,16], 18:[14,17], 19:[10,13,18], 20:[19,13]}
    integrations = {
        5: ("Integrate read_manifest, select_changes, render_notes and bundle_filename predecessor artifacts. Expose build_bundle(manifest_path, changes) returning filename and notes.", "from bundle.task_01 import read_manifest\nfrom bundle.task_02 import select_changes\nfrom bundle.task_03 import render_notes\nfrom bundle.task_04 import bundle_filename", "self.assertEqual(build_bundle('manifest.json', [{'package':'core','title':'Fix'}]), {'filename':'widget-1.0.json','notes':'- Fix\\n'})\n        self.assertEqual(build_bundle('manifest.json', [] )['notes'], '')"),
        10: ("Expose validate_bundle_result(bundle) and consume validate_manifest/build_bundle predecessor APIs; return True for a built valid result and reject malformed outputs.", "from bundle.task_05 import build_bundle\nfrom bundle.task_06 import validate_manifest", "self.assertTrue(validate_bundle_result({'filename':'widget-1.0.json','notes':'- Fix\\n'}))\n        self.assertTrue(validate_manifest({'name':'widget','version':'1.0','packages':['core']}))\n        self.assertEqual(build_bundle('manifest.json', []), {'filename':'widget-1.0.json','notes':''})\n        self.assertRaises(ValueError, validate_bundle_result, {'filename':'x.txt','notes':''})"),
        19: ("Compose build_bundle, validate_bundle_result, release_plan and release_digest from predecessor modules. Return filename, dependency order, notes digest and validation flag.", "from bundle.task_05 import build_bundle\nfrom bundle.task_10 import validate_bundle_result\nfrom bundle.task_14 import release_plan\nfrom bundle.task_16 import release_digest", "bundle = build_bundle('manifest.json', [{'package':'core','title':'Fix'}])\n        self.assertTrue(validate_bundle_result(bundle))\n        self.assertEqual(release_plan({'core':[]}), ['core'])\n        self.assertEqual(release_digest(bundle['notes']), hashlib.sha256('- Fix\\n'.encode()).hexdigest())\n        assembled = assemble_release('manifest.json', [{'package':'core','title':'Fix'}], {'core':[]})\n        self.assertTrue(assembled['valid'])\n        self.assertEqual(assembled['order'], ['core'])"),
        20: ("Expose publish_report(assembled, summary), composing assemble_release and package_summary predecessor APIs; reject unvalidated results and return the stable release report.", "from bundle.task_19 import assemble_release\nfrom bundle.task_13 import package_summary", "self.assertEqual(publish_report({'filename':'widget-1.0.json','order':['core'],'digest':'abc','valid':True}, {'packages':['core'],'count':1}), {'filename':'widget-1.0.json','packages':['core'],'order':['core'],'digest':'abc'})\n        assembled = assemble_release('manifest.json', [{'package':'core','title':'Fix'}], {'core':[]})\n        summary = package_summary({'core': [{'package':'core','title':'Fix'}]})\n        self.assertEqual(publish_report(assembled, summary), {'filename':'widget-1.0.json','packages':['core'],'order':['core'],'digest':hashlib.sha256('- Fix\\n'.encode()).hexdigest()})\n        self.assertRaises(ValueError, publish_report, {'valid':False}, {})"),
    }
    coupled_root = output / "coupled"
    coupled_rows = scaffold(coupled_root, bundle_rows, deps, integrations)
    bundle_dir = coupled_root / "release-bundle"
    (bundle_dir / "manifest.json").write_text('{"name":"widget","version":"1.0","packages":["core"]}\n')
    coupled_sha = init_repo(bundle_dir, "release-bundle", env)

    workloads = []
    for bank_name, task_rows, shas in (("independent", rows, independent_shas), ("coupled", coupled_rows, {"release-bundle": coupled_sha})):
        for topology in ("flat", "hierarchical"):
            for count in (1, 5, 10, 20):
                repo_ids = sorted({t["repository"] for t in task_rows[:count]})
                workloads.append({"workload_id": f"{bank_name}-{topology}-{count}", "bank": bank_name, "topology": topology,
                                  "accepted_tasks": count, "task_ids": [t["task_id"] for t in task_rows[:count]],
                                  "repositories": repo_ids, "seed_shas": {r: shas[r] for r in repo_ids}, "tasks": task_rows[:count]})
    manifest = {"schema": "multplx-phase12-live-workloads.v1", "seed_timestamp": "2001-01-01T00:00:00Z",
                "description": "Deterministic local Git feature seeds and failing semantic golden tests; not a benchmark runner.", "workloads": workloads}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = generate(args.output.resolve())
    print(f"wrote {len(result['workloads'])} workloads to {args.output.resolve() / 'manifest.json'}")


if __name__ == "__main__":
    main()
