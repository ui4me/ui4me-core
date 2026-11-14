"""Public docs must be navigable, consistent with the shipped API, and their examples must run."""
from pathlib import Path
import os
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from run_example import checkout_environment  # noqa: E402

PUBLIC = sorted({
    'README.md', 'CONTRIBUTING.md', 'tools/README.md',
    'examples/python/README.md', 'powershell/examples/README.md', 'powershell/UI4ME.Core/README.md',
    *(p.relative_to(ROOT).as_posix() for p in (ROOT / 'docs').rglob('*.md')),
    *(p.relative_to(ROOT).as_posix() for p in (ROOT / 'skills').rglob('*.md')),
})
# Shipped inside packages, where repository-relative links cannot resolve.
STANDALONE = ('docs/publish/pypi.md', 'powershell/UI4ME.Core/README.md', 'skills/ui4me-python/SKILL.md',
              'skills/ui4me-python/references/python-api.md', 'skills/ui4me-python/references/projects.md',
              'skills/ui4me-python/references/css-and-security.md', 'skills/ui4me-powershell/SKILL.md')
REPO_PREFIXES = ('https://github.com/ui4me/ui4me-core/blob/main/',
                 'https://github.com/ui4me/ui4me-core/tree/main/',
                 'https://raw.githubusercontent.com/ui4me/ui4me-core/main/')
LINK = re.compile(r'\]\(([^)\s]+)\)|(?:href|src)="([^"]+)"')


def text(name):
    return (ROOT / name).read_text(encoding='utf-8')


def prose(body):
    """The document without fenced or inline code, whose URLs are examples, not links."""
    body = re.sub(r'^```.*?^```', '', body, flags=re.M | re.S)
    return re.sub(r'`[^`\n]*`', '', body)


def code_blocks(body, language):
    return re.findall(r'^```' + language + r'[ \t]*\n(.*?)^```', body, re.M | re.S)


def built_host():
    name = 'ui4me-host.exe' if os.name == 'nt' else 'ui4me-host'
    for mode in ('release', 'debug'):
        candidate = ROOT / 'target' / mode / name
        if candidate.is_file():
            return str(candidate)
    return None


class LinkTests(unittest.TestCase):
    def test_local_links_resolve(self):
        failures = []
        for name in PUBLIC:
            body = prose(text(name))
            for match in LINK.finditer(body):
                target = (match.group(1) or match.group(2)).split('#', 1)[0].split('?', 1)[0].strip('<>')
                if not target or target.startswith(('https://', 'http://', 'mailto:')):
                    for prefix in REPO_PREFIXES:
                        if target.startswith(prefix) and not (ROOT / target[len(prefix):]).exists():
                            failures.append((name, 'missing in repository', target))
                    continue
                resolved = (ROOT / name).parent.joinpath(target).resolve()
                if name in STANDALONE and not (name.startswith('skills/') and resolved.exists() and
                                               resolved.is_relative_to((ROOT / '/'.join(name.split('/')[:2])).resolve())):
                    # A skill is installed as one folder; anything else must use absolute links.
                    failures.append((name, 'relative link in packaged document', target))
                elif not (ROOT / name).parent.joinpath(target).exists():
                    failures.append((name, 'missing', target))
        self.assertEqual(failures, [])

    def test_no_references_to_retired_documents(self):
        retired = re.compile(r'CORE_1\.0_|PROTOCOL_DOCUMENTS|CAPABILITY_SCOPE\.md|docs/STATUS\.md|RC2_|BASELINE_|'
                             r'guides/quickstart\.md|guides/powershell\.md|PUBLISHING\.md|RELEASE_NOTES_1\.0\.0')
        hits = [(name, m.group(0)) for name in PUBLIC for m in retired.finditer(text(name))
                if 'git show v1.0.0:' not in text(name)[max(0, m.start() - 40):m.start()]]
        self.assertEqual(hits, [])


class AudienceTests(unittest.TestCase):
    def test_pypi_description_is_the_packaged_readme(self):
        project = tomllib.loads(text('pyproject.toml'))['project']
        self.assertEqual(project['readme'], {'file': 'docs/publish/pypi.md', 'content-type': 'text/markdown'})
        body = text('docs/publish/pypi.md')
        self.assertIn('raw.githubusercontent.com/ui4me/ui4me-core/main/docs/images/desktop-workspace.png', body)
        self.assertNotIn('<details', body)  # PyPI's sanitizer is not GitHub's
        self.assertNotIn('Install-Module', body)  # Python audience only
        self.assertIn('Application.start(', body)
        self.assertIn('uv add ui4me-core', body)

    def test_github_readme_layout(self):
        body = text('README.md')
        summaries = re.findall(r'<details(\s[^>]*)?>\s*<summary>(.*?)</summary>', body, re.S)
        self.assertGreaterEqual(len(summaries), 5)
        self.assertFalse([attributes for attributes, _ in summaries if attributes and 'open' in attributes])
        titles = ' '.join(title for _, title in summaries)
        for topic in ('Python on Linux', 'Python on Windows', 'PowerShell', 'Documentation', 'coding agent'):
            self.assertIn(topic, titles)
        self.assertRegex(body, r'<a href="docs/images/desktop-workspace\.png"><img src="docs/images/desktop-workspace\.png" width="\d+"')
        self.assertLess(body.index('## Your first Python app'), body.index('## Your first PowerShell app'))
        self.assertEqual(len(code_blocks(body, 'python')), 1)

    def test_gallery_readme_is_for_powershell_users(self):
        body = text('powershell/UI4ME.Core/README.md')
        for needed in ('Install-Module UI4ME.Core', 'Start-UI4MEScript', 'pwsh -File', 'powershell.exe'):
            self.assertIn(needed, body)
        self.assertNotIn('pip install', body)

    def test_beginner_python_examples_use_named_callbacks(self):
        for name in ('README.md', 'docs/publish/pypi.md', 'docs/guides/quickstart-python.md',
                     'examples/python/hello.py', 'examples/python/notes/app.py'):
            body = text(name)
            self.assertNotIn('lambda', body, name)
            self.assertNotIn('from_environment', body if name.endswith('.py') else
                             ''.join(code_blocks(body, 'python')), name)

    def test_python_skill_teaches_direct_start_and_keeps_manifest_launch(self):
        skill = text('skills/ui4me-python/SKILL.md')
        self.assertTrue(skill.startswith('---\nname: ui4me-python\n'))
        self.assertIn('Application.start(', skill)
        self.assertIn('Application.from_environment()', skill)
        self.assertIn('ui4me-host run --app', skill)
        self.assertLess(skill.index('Application.start('), skill.index('Application.from_environment()'))
        powershell = text('skills/ui4me-powershell/SKILL.md')
        self.assertTrue(powershell.startswith('---\nname: ui4me-powershell\n'))
        self.assertIn('Start-UI4MEScript', powershell)

    def test_python_distribution_declares_skills_and_generator(self):
        data = tomllib.loads(text('pyproject.toml'))
        scripts = data['project']['scripts']
        self.assertEqual(scripts['ui4me-skill'], 'ui4me_core.skills:main')
        self.assertEqual(scripts['ui4me-new'], 'ui4me_core.scaffold:main')
        self.assertIn('graft skills', text('MANIFEST.in'))
        self.assertIn('skill_source = ROOT / "skills" / "ui4me-python"', text('setup.py'))

    def test_real_screenshots(self):
        import struct
        for name in ('desktop-workspace.png', 'live-bitmap.png'):
            payload = (ROOT / 'docs/images' / name).read_bytes()
            self.assertTrue(payload.startswith(b'\x89PNG\r\n\x1a\n'))
            width, height = struct.unpack_from('>II', payload, 16)
            self.assertGreater(width, 550)
            self.assertGreater(height, 400)


class ExampleCodeTests(unittest.TestCase):
    def test_python_blocks_compile(self):
        failures = []
        for name in PUBLIC:
            for index, block in enumerate(code_blocks(text(name), 'python')):
                try:
                    compile(textwrap.dedent(block), f'{name}[{index}]', 'exec')
                except SyntaxError as error:
                    failures.append((name, index, str(error)))
        self.assertEqual(failures, [])

    @unittest.skipUnless(shutil.which('pwsh'), 'PowerShell 7 is not installed')
    def test_powershell_blocks_parse(self):
        with tempfile.TemporaryDirectory() as folder:
            count = 0
            for name in PUBLIC:
                for index, block in enumerate(code_blocks(text(name), 'powershell')):
                    Path(folder, f'{count:04d}.ps1').write_text(f'# {name}[{index}]\n' + block, encoding='utf-8-sig')
                    count += 1
            checker = Path(folder, 'check-syntax.ps1.txt')
            checker.write_text(
                "$bad = foreach ($f in Get-ChildItem -LiteralPath $args[0] -Filter *.ps1) { $e = $null; "
                "$null = [Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$null, [ref]$e); "
                "if ($e) { (Get-Content -LiteralPath $f.FullName -TotalCount 1) + ': ' + $e[0].Message } }\n"
                "$bad\nexit @($bad).Count\n", encoding='utf-8')
            runner = checker.with_name('check-syntax.ps1')
            checker.rename(runner)
            result = subprocess.run(['pwsh', '-NoProfile', '-NonInteractive', '-File', str(runner), folder],
                                    capture_output=True, text=True, timeout=120)
        self.assertGreater(count, 10)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_documented_batch_ops_are_accepted_by_the_host(self):
        source = text('crates/ui4me-renderer/src/document.rs')
        enum = source.split('pub enum DomOp {', 1)[1].split('\n}\n', 1)[0]
        variants = re.findall(r'^    ([A-Z][A-Za-z]+) \{', enum, re.M)
        allowed = {re.sub(r'(?<!^)([A-Z])', r'_\1', v).lower() for v in variants}
        self.assertIn('replace_children_html', allowed)
        sys.path.insert(0, str(ROOT / 'python'))
        from ui4me_core import scaffold
        texts = {name: text(name) for name in PUBLIC}
        texts['scaffold'] = '\n'.join(scaffold.templates('Check', '/venv/bin/python').values())
        bad = [(name, op) for name, body in texts.items()
               for op in re.findall(r'''["']op["']\s*:\s*["']([a-z_]+)["']''', body) if op not in allowed]
        self.assertEqual(bad, [])


@unittest.skipUnless(built_host(), 'build the host first: cargo build -p ui4me-host')
class RunnableExampleTests(unittest.TestCase):
    """Run the beginner examples end to end through the real host in headless test
    mode. A wrapper asks each app to quit once it has announced readiness."""

    WRAPPER = textwrap.dedent('''\
        import runpy, sys, threading
        from ui4me_core import Application
        original = Application.run
        def run(self):
            threading.Timer(0.3, self.request_quit).start()
            return original(self)
        Application.run = run
        target = sys.argv.pop(1)
        sys.argv[0] = target
        runpy.run_path(target, run_name="__main__")
        ''')

    def run_example(self, script: Path):
        with tempfile.TemporaryDirectory() as folder:
            wrapper = Path(folder) / 'run_example_headless.py'
            wrapper.write_text(self.WRAPPER, encoding='utf-8')
            env = checkout_environment({k: v for k, v in os.environ.items() if not k.startswith('UI4ME_')})
            env.update({'UI4ME_TEST_MODE': '1', 'UI4ME_HOST_BINARY': built_host()})
            result = subprocess.run([sys.executable, str(wrapper), str(script)], cwd=script.parent, env=env,
                                    capture_output=True, text=True, timeout=120)
        self.assertEqual(result.returncode, 0, f'{script}\n{result.stdout}\n{result.stderr}')

    def test_example_files(self):
        for script in (ROOT / 'examples/python/hello.py', ROOT / 'examples/python/notes/app.py'):
            with self.subTest(script=script.name):
                self.run_example(script)

    def test_readme_pypi_and_quickstart_apps(self):
        for name in ('README.md', 'docs/publish/pypi.md', 'docs/guides/quickstart-python.md'):
            blocks = [b for b in code_blocks(text(name), 'python') if 'Application.start(' in b and '__main__' in b]
            self.assertTrue(blocks, name)
            with tempfile.TemporaryDirectory() as folder:
                for index, block in enumerate(blocks):
                    with self.subTest(document=name, block=index):
                        script = Path(folder) / f'app{index}.py'
                        script.write_text(block, encoding='utf-8')
                        if 'index.html' in block:
                            quickstart = text(name)
                            Path(folder, 'index.html').write_text(code_blocks(quickstart, 'html')[0], encoding='utf-8')
                            Path(folder, 'style.css').write_text(code_blocks(quickstart, 'css')[0], encoding='utf-8')
                        self.run_example(script)


if __name__ == '__main__':
    unittest.main()
