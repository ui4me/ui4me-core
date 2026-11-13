"""Test the installed project generator and TOML/backend API contract."""
import ast
import sys
import tempfile
import tomllib
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'python'))
from ui4me_core import scaffold


class ScaffoldTests(unittest.TestCase):
    def test_project_generated_without_running_rust(self):
        with tempfile.TemporaryDirectory() as d:
            dest = Path(d) / 'My-Workspace'
            files=scaffold.create_project(dest, name='My Workspace', python='/custom/.venv/bin/python')
            self.assertEqual(len(files), 4)
            manifest=tomllib.loads((dest/'app.toml').read_text())
            self.assertEqual(manifest['app']['id'], 'org.ui4me.examples.my_workspace')
            self.assertEqual(manifest['backend']['program'], '/custom/.venv/bin/python')
            self.assertEqual(manifest['backend']['args'], ['{app_dir}/app.py'])
            ast.parse((dest/'app.py').read_text())
            self.assertIn('ui4me_core import Application',(dest/'app.py').read_text())
            self.assertIn('id="add-item"',(dest/'index.html').read_text())
            with self.assertRaises(FileExistsError):
                scaffold.create_project(dest, name='again')

    def test_windows_python_path_toml_escaping(self):
        template=scaffold.templates('Cool GUI',r'C:\Users\Me\Envs\test\Scripts\python.exe')
        self.assertEqual(tomllib.loads(template['app.toml'])['backend']['program'],
                         r'C:\Users\Me\Envs\test\Scripts\python.exe')

    def test_invalid_app_id_and_escaping(self):
        with self.assertRaises(ValueError):
            scaffold.app_slug('123')
        with self.assertRaises(ValueError):
            scaffold.app_slug('')
        x=scaffold.templates('Quotes " and slash \\', '/venv/bin/python')
        self.assertEqual(tomllib.loads(x['app.toml'])['app']['name'], 'Quotes " and slash \\')

    def test_unicode_and_quote_names_generate_valid_code(self):
        for name in ('Say """hi""" <b>&', 'Trail \\', 'Café Ünïcode', '日本語アプリ'):
            with self.subTest(name=name):
                files=scaffold.templates(name, r'C:\venv\Scripts\python.exe')
                tree=ast.parse(files['app.py'])
                consts=[n.value.value for n in tree.body if isinstance(n, ast.Assign)
                        and getattr(n.targets[0], 'id', '')=='APP_NAME']
                self.assertEqual(consts,[name])
                self.assertEqual(tomllib.loads(files['app.toml'])['app']['name'], name)
                self.assertNotIn('<b>', files['index.html'])
        self.assertEqual(scaffold.app_slug('Café Ünïcode'), 'cafe-unicode')
        self.assertTrue(scaffold.app_slug('日本語アプリ').startswith('app-'))

    def test_printed_run_command_is_quoted(self):
        self.assertEqual(scaffold.shell_argument('/tmp/a b/app.toml', windows=False), "'/tmp/a b/app.toml'")
        self.assertEqual(scaffold.shell_argument(r'C:\Apps\demo\app.toml', windows=True), r'C:\Apps\demo\app.toml')
        self.assertEqual(scaffold.shell_argument(r"C:\My Apps\it's\app.toml", windows=True), r"'C:\My Apps\it''s\app.toml'")

    def test_cli_output_survives_a_legacy_console_code_page(self):
        import io
        from unittest import mock
        with tempfile.TemporaryDirectory() as d:
            out=io.TextIOWrapper(io.BytesIO(), encoding='cp1252')
            with mock.patch.object(sys, 'stdout', out):
                rc=scaffold.main([str(Path(d)/'Café ✓'), '--python', 'python'])
            self.assertEqual(rc, 0)
            out.flush()
            self.assertIn(b'\\u2713', out.buffer.getvalue())
            self.assertTrue((Path(d)/'Café ✓'/'app.toml').is_file())

    def test_empty_directory_allowed_and_nonempty_refused(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            empty=root/'empty'
            empty.mkdir()
            scaffold.create_project(empty, name='Working')
            nonempty=root/'occupied'
            nonempty.mkdir()
            (nonempty/'keep.txt').write_text('do not erase')
            with self.assertRaises(FileExistsError):
                scaffold.create_project(nonempty)
            self.assertEqual((nonempty/'keep.txt').read_text(),'do not erase')


if __name__ == '__main__':
    unittest.main()
