"""Waifu.im v7 network client; no GUI calls belong in this module."""
import io
import math
import time
import requests
from PIL import Image


class WaifuClient:
    BASE_URL = "https://api.waifu.im"
    HEADERS = {"Accept-Version": "v7", "User-Agent": "waifu-downloader/1.0"}

    def __init__(self, get=None, sleep=time.sleep):
        self.get = get or requests.get
        self.sleep = sleep

    def _request(self, url, **kwargs):
        for attempt in range(4):
            response = self.get(url, **kwargs)
            if response.status_code != 429 or attempt == 3:
                try:
                    response.raise_for_status()
                except requests.HTTPError:
                    response.close()
                    raise
                return response
            try:
                delay = float(response.headers.get("Retry-After", 2 ** attempt))
                if not math.isfinite(delay) or delay < 0:
                    raise ValueError()
            except (TypeError, ValueError):
                delay = 2 ** attempt
            response.close()
            self.sleep(delay)

    def tags(self):
        result = set()
        page = 1
        while True:
            response = self._request(
                self.BASE_URL + "/tags", headers=self.HEADERS,
                params={"Page": page, "PageSize": 100}, timeout=15)
            with response:
                data = response.json()
            items = data.get("items") if isinstance(data, dict) else data
            if not isinstance(items, list):
                raise ValueError("Некорректный список тегов API")
            result.update(item["slug"] for item in items
                          if isinstance(item, dict) and isinstance(item.get("slug"), str)
                          and item["slug"] and "/" not in item["slug"]
                          and "\\" not in item["slug"] and item["slug"] not in (".", ".."))
            if not isinstance(data, dict) or not data.get("hasNextPage"):
                break
            page += 1
        if not result:
            raise ValueError("API вернул пустой список тегов")
        return sorted(result)

    def image(self, mode, tag, custom_url=None):
        kwargs = {"timeout": 15}
        if not custom_url:
            kwargs.update(headers=self.HEADERS, params={
                "IncludedTags": tag, "IsNsfw": "True" if mode == "NSFW" else "False",
                "PageSize": 1})
        else:
            kwargs["headers"] = self.HEADERS
        response = self._request(custom_url or self.BASE_URL + "/images", **kwargs)
        with response:
            data = response.json()
        if not isinstance(data, dict):
            raise ValueError("Некорректный ответ API")
        if "items" in data:
            items = data["items"]
            if not isinstance(items, list):
                raise ValueError("Некорректный список изображений")
            if not items:
                raise ValueError("Для выбранного тега и режима изображений нет")
            item = items[0]
            url = item.get("url") if isinstance(item, dict) else None
        else:
            url = data.get("url") if custom_url else None
        if not isinstance(url, str) or not url.startswith(("https://", "http://")):
            raise ValueError("API не вернул ссылку на изображение")
        # Do not forward API headers to the image host.
        response = self._request(url, timeout=30)
        with response:
            content = response.content
        with Image.open(io.BytesIO(content)) as image:
            return image.convert("RGBA")
