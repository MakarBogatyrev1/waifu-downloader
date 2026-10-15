import io
import json
import os
import queue
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch
import requests
from PIL import Image
from waifu_client import WaifuClient
from waifu import App, GalleryManager, SettingsManager


class Response:
    def __init__(self, data=None, content=b"", status=200, headers=None):
        self.data, self.content, self.status_code = data, content, status
        self.headers = headers or {}
        self.closed = False
    def json(self):
        if isinstance(self.data, Exception):
            raise self.data
        return self.data
    def raise_for_status(self):
        if self.status_code >= 400:
            raise requests.HTTPError(str(self.status_code))
    def close(self):
        self.closed = True
    def __enter__(self):
        return self
    def __exit__(self, *args):
        self.close()


def png(size=(1600, 1200)):
    output = io.BytesIO()
    Image.new("RGBA", size, "blue").save(output, "PNG")
    return output.getvalue()


class ClientTests(unittest.TestCase):
    def client(self, responses):
        self.calls = []
        self.sleeps = []
        def get(url, **kwargs):
            self.calls.append((url, kwargs))
            response = responses.pop(0)
            if isinstance(response, Exception):
                raise response
            return response
        return WaifuClient(get, self.sleeps.append)

    def test_modes_and_full_resolution(self):
        for mode, flag in [("SFW", "False"), ("NSFW", "True")]:
            c = self.client([Response({"items": [{"url": "https://cdn.test/a.png"}]}),
                             Response(content=png())])
            self.assertEqual(c.image(mode, "maid").size, (1600, 1200))
            url, args = self.calls[0]
            self.assertEqual(url, "https://api.waifu.im/images")
            self.assertEqual(args["params"], {"IncludedTags": "maid", "IsNsfw": flag, "PageSize": 1})
            self.assertEqual(args["headers"]["Accept-Version"], "v7")
            self.assertNotIn("headers", self.calls[1][1])

    def test_tags_pagination_and_path_safety(self):
        c = self.client([
            Response({"items": [{"slug": "waifu"}, {"slug": "../bad"}], "hasNextPage": True}),
            Response({"items": [{"slug": "maid"}, {"slug": "waifu"}], "hasNextPage": False})])
        self.assertEqual(c.tags(), ["maid", "waifu"])
        self.assertEqual(self.calls[1][1]["params"]["Page"], 2)

    def test_custom_url_legacy_and_v7(self):
        for data in [{"url": "https://cdn.test/a.png"},
                     {"items": [{"url": "https://cdn.test/a.png"}]}]:
            c = self.client([Response(data), Response(content=png((20, 30)))])
            self.assertEqual(c.image("SFW", "waifu", "https://custom.test").size, (20, 30))
            self.assertNotIn("params", self.calls[0][1])

    def test_bad_responses(self):
        for data in [{}, [], {"items": []}, {"items": {}}, {"items": [None]},
                     {"items": [{"url": "file:///x"}]}, ValueError("JSON")]:
            with self.subTest(data=data):
                c = self.client([Response(data)])
                with self.assertRaises(ValueError):
                    c.image("SFW", "waifu")

    def test_bad_tags(self):
        for data in [{}, {"items": []}, {"items": [{}]}]:
            with self.assertRaises(ValueError):
                self.client([Response(data)]).tags()

    def test_rate_limit_retry_and_exhaustion(self):
        r = Response(status=429, headers={"Retry-After": "0.25"})
        c = self.client([r, Response({"items": [{"slug": "waifu"}]})])
        self.assertEqual(c.tags(), ["waifu"])
        self.assertEqual(self.sleeps, [0.25])
        self.assertTrue(r.closed)
        c = self.client([Response(status=429) for _ in range(4)])
        with self.assertRaises(requests.HTTPError):
            c.tags()
        self.assertEqual(self.sleeps, [1, 2, 4])
        self.assertEqual(len(self.calls), 4)

    def test_invalid_retry_after(self):
        c = self.client([Response(status=429, headers={"Retry-After": "nan"}),
                         Response({"items": [{"slug": "waifu"}]})])
        c.tags()
        self.assertEqual(self.sleeps, [1])

    def test_network_and_server_errors(self):
        for response in [requests.Timeout("timeout"), Response(status=500)]:
            with self.assertRaises(requests.RequestException):
                self.client([response]).image("SFW", "waifu")
            self.assertEqual(len(self.calls), 1)

    def test_corrupt_image(self):
        c = self.client([Response({"items": [{"url": "https://cdn.test/a"}]}),
                         Response(content=b"not an image")])
        with self.assertRaises(OSError):
            c.image("SFW", "waifu")


class GuiTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.old_cwd = os.getcwd()
        os.chdir(self.temp.name)
        self.tags_patch = patch.object(WaifuClient, "tags", return_value=["maid", "waifu"])
        self.tags_patch.start()
        self.app = None

    def tearDown(self):
        if self.app is not None and not self.app._closed:
            self.app.on_close()
        self.tags_patch.stop()
        os.chdir(self.old_cwd)
        self.temp.cleanup()

    def open_app(self):
        self.app = App()
        self.app.withdraw()
        self.pump(lambda: self.app.tags == ["maid", "waifu"])
        return self.app

    def pump(self, condition, timeout=3):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.app.update()
            if condition():
                return
            time.sleep(0.01)
        self.fail("UI condition timed out")

    def test_legacy_settings_migration(self):
        Path("settings.json").write_text(json.dumps({
            "mode": "NSFW", "category": "neko", "save_mode": "B", "thumb_size": 96}))
        app = self.open_app()
        self.assertEqual(app.settings.mode, "NSFW")
        self.assertEqual(app.settings.category, "waifu")
        self.assertEqual(app.settings.save_mode, "B")
        self.assertEqual(app.settings.thumb_size, 96)
        saved = json.loads(Path("settings.json").read_text())
        self.assertEqual(saved["category"], "waifu")

    def test_valid_old_tag_preserved(self):
        Path("settings.json").write_text(json.dumps({"category": "maid"}))
        self.assertEqual(self.open_app().settings.category, "maid")

    def test_invalid_mode_normalized(self):
        Path("settings.json").write_text(json.dumps({"mode": "bad"}))
        self.assertEqual(self.open_app().settings.mode, "SFW")

    def test_original_saved_from_thumbnail_for_all_folders(self):
        app = self.open_app()
        original = Image.new("RGBA", (1800, 1400), "red")
        app.gallery.add_new_image(original)
        other = Image.new("RGBA", (1600, 1300), "blue")
        app.gallery.add_new_image(other)
        app.gallery.thumbnail_buttons[1].invoke()
        for mode in ["A", "B", "C"]:
            app.settings.save_mode = mode
            app.settings.custom_save_folder = str(Path("custom").resolve())
            ok, message = app.gallery.save_current_image()
            self.assertTrue(ok, message)
            files = list(app.settings.get_save_folder_for_current().glob("*.png"))
            self.assertEqual(len(files), 1)
            with Image.open(files[0]) as saved:
                self.assertEqual(saved.size, original.size)
                self.assertEqual(saved.getpixel((0, 0)), (255, 0, 0, 255))
            self.assertTrue(any(path == files[0] for _, path in app.gallery.saved_images))

    def test_background_responsive_duplicate_and_stale_result(self):
        app = self.open_app()
        entered, release = threading.Event(), threading.Event()
        calls = []
        def image(*args):
            calls.append(args)
            entered.set()
            release.wait(2)
            return Image.new("RGBA", (100, 200))
        with patch.object(app.client, "image", side_effect=image):
            app.on_next()
            self.assertTrue(entered.wait(1))
            app.on_next()
            self.assertEqual(len(calls), 1)
            self.assertEqual(app.btn_next.cget("state"), "disabled")
            heartbeat = []
            app.after(0, lambda: heartbeat.append(True))
            self.pump(lambda: bool(heartbeat))
            app.on_mode_change("NSFW")
            release.set()
            self.pump(lambda: not app._loading)
            self.assertEqual(app.gallery.new_images, [])
            self.assertEqual(app.btn_next.cget("state"), "normal")

    def test_success_and_error_restore_button(self):
        app = self.open_app()
        with patch.object(app.client, "image", return_value=Image.new("RGBA", (25, 35))):
            app.on_next()
            self.pump(lambda: not app._loading)
        self.assertEqual(app.gallery.current_image.size, (25, 35))
        with patch.object(app.client, "image", side_effect=requests.Timeout("timeout")):
            app.on_next()
            self.pump(lambda: not app._loading)
        self.assertIn("timeout", app.status_var.get())
        self.assertEqual(app.btn_next.cget("state"), "normal")

    def test_close_during_download(self):
        app = self.open_app()
        entered, release, done = threading.Event(), threading.Event(), threading.Event()
        def image(*args):
            entered.set()
            release.wait(2)
            done.set()
            return Image.new("RGBA", (10, 10))
        with patch.object(app.client, "image", side_effect=image):
            app.on_next()
            self.assertTrue(entered.wait(1))
            app.on_close()
            release.set()
            self.assertTrue(done.wait(1))
            self.assertTrue(app._closed)

    def test_tags_failure_fallback(self):
        self.tags_patch.stop()
        with patch.object(WaifuClient, "tags", side_effect=requests.Timeout("tags timeout")):
            self.app = App()
            self.app.withdraw()
            self.pump(lambda: "tags timeout" in self.app.status_var.get())
            self.assertEqual(self.app.settings.category, "waifu")
            self.assertEqual(self.app.tags, ["waifu"])


if __name__ == "__main__":
    unittest.main()
