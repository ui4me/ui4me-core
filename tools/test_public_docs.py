"""Public-facing docs must be navigable and faithful to shipped entry points."""
from pathlib import Path
import re
import sys
import tomllib
import unittest

ROOT=Path(__file__).resolve().parents[1]
PUBLIC=(
    'README.md','docs/index.md','docs/guides/quickstart.md',
    'docs/guides/agent-skills.md','docs/guides/platforms.md',
    'docs/guides/patterns.md','docs/guides/development.md',
    'docs/reference/python-api.md','docs/release/PUBLISHING.md',
    'docs/release/RELEASE_NOTES_1.0.0.md','skills/ui4me-python/SKILL.md',
    'skills/ui4me-python/references/python-api.md',
    'skills/ui4me-python/references/projects.md',
    'skills/ui4me-python/references/css-and-security.md',
)


class PublicDocsTests(unittest.TestCase):
    def test_public_local_links_resolve(self):
        failures=[]
        for name in PUBLIC:
            path=ROOT/name
            self.assertTrue(path.is_file(),name)
            body=path.read_text(encoding='utf-8')
            for match in re.finditer(r'\]\(([^)]+)\)',body):
                target=match.group(1).split('#',1)[0].split('?',1)[0].strip()
                if not target or target.startswith(('https://','http://','mailto:')):
                    continue
                if not (path.parent/target).exists():
                    failures.append((name,target))
        self.assertEqual(failures,[])

    def test_readme_links_work_on_pypi(self):
        # README.md is also the PyPI long description, where relative links
        # and images do not resolve. Repository links must name real files.
        body=(ROOT/'README.md').read_text(encoding='utf-8')
        prefixes=('https://github.com/ui4me/ui4me-core/blob/main/',
                  'https://github.com/ui4me/ui4me-core/tree/main/',
                  'https://raw.githubusercontent.com/ui4me/ui4me-core/main/')
        failures=[]
        for match in re.finditer(r'\]\(([^)]+)\)',body):
            target=match.group(1)
            if not target.startswith(('https://','http://','mailto:')):
                failures.append(('relative',target))
                continue
            for prefix in prefixes:
                if target.startswith(prefix):
                    local=target[len(prefix):].split('#',1)[0]
                    if not (ROOT/local).exists():
                        failures.append(('missing',target))
        self.assertEqual(failures,[])
        self.assertIn('raw.githubusercontent.com/ui4me/ui4me-core/main/docs/images/desktop-workspace.png',body)

    def test_documented_batch_ops_are_accepted_by_the_host(self):
        source=(ROOT/'crates/ui4me-renderer/src/document.rs').read_text(encoding='utf-8')
        enum=source.split('pub enum DomOp {',1)[1].split('\n}\n',1)[0]
        variants=re.findall(r'^    ([A-Z][A-Za-z]+) \{',enum,re.M)
        allowed={re.sub(r'(?<!^)([A-Z])',r'_\1',v).lower() for v in variants}
        self.assertIn('replace_children_html',allowed)
        sys.path.insert(0,str(ROOT/'python'))
        from ui4me_core import scaffold
        texts={name:(ROOT/name).read_text(encoding='utf-8') for name in PUBLIC}
        texts['scaffold']='\n'.join(scaffold.templates('Check','/venv/bin/python').values())
        bad=[]
        for name,body in texts.items():
            for op in re.findall(r'''["']op["']\s*:\s*["']([a-z_]+)["']''',body):
                if op not in allowed:
                    bad.append((name,op))
        self.assertEqual(bad,[])

    def test_real_verified_screenshots(self):
        import struct
        for name in ('desktop-workspace.png','live-bitmap.png'):
            path=ROOT/'docs/images'/name
            self.assertTrue(path.is_file())
            payload=path.read_bytes()
            self.assertTrue(payload.startswith(b'\x89PNG\r\n\x1a\n'))
            width,height=struct.unpack_from('>II',payload,16)
            self.assertGreater(width,550)
            self.assertGreater(height,400)

    def test_python_distribution_declares_skills_and_generator(self):
        data=tomllib.loads((ROOT/'pyproject.toml').read_text())
        scripts=data['project']['scripts']
        self.assertEqual(scripts['ui4me-skill'],'ui4me_core.skills:main')
        self.assertEqual(scripts['ui4me-new'],'ui4me_core.scaffold:main')
        self.assertIn('graft skills',(ROOT/'MANIFEST.in').read_text())
        source=(ROOT/'setup.py').read_text()
        self.assertIn('skill_source = ROOT / "skills" / "ui4me-python"',source)
        skill=(ROOT/'skills/ui4me-python/SKILL.md').read_text()
        self.assertTrue(skill.startswith('---\nname: ui4me-python\n'))
        self.assertIn('Application.from_environment()',skill)
        self.assertIn('ui4me-host run --app',skill)


if __name__=='__main__':
    unittest.main()
