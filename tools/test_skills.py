"""Cross-platform agent-skill tests; all writes occur under temporary directories."""
import json
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'python'))
from ui4me_core import skills


class SkillInstallerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = skills.skill_source()
        self.assertTrue((self.source / 'SKILL.md').is_file())

    def test_install_status_update_and_uninstall(self):
        target = self.root / '.claude' / 'skills' / skills.SKILL_NAME
        self.assertEqual(skills.skill_status(target), 'missing')
        self.assertEqual(skills.install_skill(self.source, target), 'installed')
        self.assertEqual(skills.skill_status(target), 'installed')
        ownership = json.loads((target / skills.MARKER).read_text())
        self.assertEqual(ownership['name'], 'ui4me-python')
        self.assertIn('SKILL.md', ownership['files'])
        self.assertEqual(skills.install_skill(self.source, target), 'updated')
        self.assertEqual(skills.uninstall_skill(target), 'removed')
        self.assertEqual(skills.skill_status(target), 'missing')

    def test_unmanaged_is_never_overwritten_or_removed(self):
        target = self.root / '.agents' / 'skills' / skills.SKILL_NAME
        target.mkdir(parents=True)
        (target / 'note.txt').write_text('Other personal skill')
        with self.assertRaises(FileExistsError):
            skills.install_skill(self.source, target)
        with self.assertRaises(FileExistsError):
            skills.uninstall_skill(target)
        self.assertEqual((target / 'note.txt').read_text(), 'Other personal skill')

    def test_modified_skill_requires_explicit_force(self):
        target = self.root / '.agents' / 'skills' / skills.SKILL_NAME
        skills.install_skill(self.source, target)
        (target / 'SKILL.md').write_text('Hand edited')
        self.assertEqual(skills.skill_status(target), 'modified')
        with self.assertRaises(FileExistsError):
            skills.install_skill(self.source, target)
        self.assertEqual((target / 'SKILL.md').read_text(), 'Hand edited')
        skills.install_skill(self.source, target, force=True)
        self.assertEqual(skills.skill_status(target), 'installed')

    def test_reject_symlinked_destinations(self):
        if not hasattr(Path, 'symlink_to'):
            self.skipTest('No symlink API')
        outside = self.root / 'outside'
        outside.mkdir()
        alias = self.root / '.claude'
        try:
            alias.symlink_to(outside, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest('No symlink permission')
        with self.assertRaises(ValueError):
            skills.install_skill(self.source, alias / 'skills' / skills.SKILL_NAME)

    def test_agent_locations_are_distinct_and_portable(self):
        names = skills.agent_names('all')
        self.assertEqual(len(names), 6)
        home = skills.locations(names, 'user', home=self.root)
        self.assertEqual(home['claude'], self.root / '.claude/skills/ui4me-python')
        self.assertEqual(home['codex'], self.root / '.agents/skills/ui4me-python')
        self.assertEqual(home['opencode'], self.root / '.config/opencode/skills/ui4me-python')
        project = skills.locations(('claude','codex'), 'project', project=self.root)
        self.assertEqual(project['codex'], self.root / '.agents/skills/ui4me-python')
        with self.assertRaises(ValueError):
            skills.agent_names('unknown')

    def test_cli_dry_run_and_project_scope(self):
        with patch('ui4me_core.skills.Path.home', return_value=self.root):
            self.assertEqual(skills.main(['install','--agents','claude','--dry-run']), 0)
            self.assertEqual(skills.main(['install','--agents','claude']), 0)
            self.assertEqual(skills.main(['status','--agents','claude']), 0)
            self.assertEqual(skills.main(['uninstall','--agents','claude']), 0)
        self.assertFalse((self.root / '.claude/skills/ui4me-python').exists())

    def test_cli_output_survives_a_legacy_console_code_page(self):
        import io
        project = self.root / 'Projekt ✓'
        project.mkdir()
        out = io.TextIOWrapper(io.BytesIO(), encoding='cp1252')
        with patch.object(sys, 'stdout', out):
            rc = skills.main(['install', '--scope', 'project', '--project', str(project), '--agents', 'claude'])
        self.assertEqual(rc, 0)
        self.assertTrue((project / '.claude' / 'skills' / skills.SKILL_NAME / 'SKILL.md').is_file())


if __name__ == '__main__':
    unittest.main()
