from pathlib import Path
import tempfile
import unittest
from check_docs import check_files


class DocsTests(unittest.TestCase):
    def test_encoded_local_asset_and_external_links(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            (root / "my image.png").write_bytes(b"fixture")
            doc = root / "README.md"
            doc.write_text('![Image](<my%20image.png>)\n[Web](https://example.org/missing)\n[Heading](#local)\n', encoding="utf-8")
            self.assertEqual(check_files(root, [doc]), (1, []))

    def test_missing_and_outside_repository_link_fail(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            doc = root / "README.md"
            doc.write_text('[Missing](missing.md)\n[Outside](../outside.md)\n', encoding="utf-8")
            count, errors = check_files(root, [doc])
            self.assertEqual(count, 2)
            self.assertEqual(len(errors), 2)

    def test_fenced_examples_are_not_rendered_links(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            doc = root / "README.md"
            doc.write_text('```md\n[Example](missing.md)\n```\n', encoding="utf-8")
            self.assertEqual(check_files(root, [doc]), (0, []))
