"""
Tests for calibrectl.metadata
"""

import subprocess
from unittest.mock import MagicMock

from calibrectl.metadata import fetch_metadata_from_calibre

OPF_SAMPLE = """<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf"
         xmlns:dc="http://purl.org/dc/elements/1.1/"
         xmlns:opf="http://www.idpf.org/2007/opf">
  <metadata>
    <dc:title>Python Cookbook</dc:title>
    <dc:creator opf:role="aut">David Beazley</dc:creator>
    <dc:creator opf:role="aut">Brian K. Jones</dc:creator>
    <dc:publisher>O'Reilly Media</dc:publisher>
    <dc:date>2013-05-10</dc:date>
    <dc:description>Recipes for Python 3 programming.</dc:description>
    <dc:identifier opf:scheme="ISBN">9781449340377</dc:identifier>
    <dc:subject>Python</dc:subject>
    <dc:subject>Programming</dc:subject>
    <dc:language>en</dc:language>
  </metadata>
</package>
"""


class TestFetchMetadataFromCalibre:
    def test_success_by_isbn(self, mocker):
        mock_run = mocker.patch("calibrectl.metadata.subprocess.run")
        mock_run.return_value = MagicMock(returncode=0, stdout=OPF_SAMPLE)

        result = fetch_metadata_from_calibre(isbn="9781449340377")

        assert result is not None
        assert result["title"] == "Python Cookbook"
        assert "David Beazley" in result["authors"]
        assert result["publisher"] == "O'Reilly Media"
        assert result["pubdate"] == "2013-05-10"
        assert result["isbn"] == "9781449340377"
        assert "Python" in result["tags"]
        assert result["language"] == "en"

    def test_success_by_title(self, mocker):
        mock_run = mocker.patch("calibrectl.metadata.subprocess.run")
        mock_run.return_value = MagicMock(returncode=0, stdout=OPF_SAMPLE)

        result = fetch_metadata_from_calibre(title="Python Cookbook", authors=["Beazley"])

        assert result is not None
        assert result["title"] == "Python Cookbook"

    def test_nonzero_exit_returns_none(self, mocker):
        mock_run = mocker.patch("calibrectl.metadata.subprocess.run")
        mock_run.return_value = MagicMock(returncode=1, stdout="")

        result = fetch_metadata_from_calibre(isbn="9781449340377")
        assert result is None

    def test_calibredb_not_found_returns_none(self, mocker):
        mock_run = mocker.patch("calibrectl.metadata.subprocess.run")
        mock_run.side_effect = FileNotFoundError("No such file")

        result = fetch_metadata_from_calibre(isbn="9781449340377")
        assert result is None

    def test_timeout_returns_none(self, mocker):
        mock_run = mocker.patch("calibrectl.metadata.subprocess.run")
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="fetch-ebook-metadata", timeout=30)

        result = fetch_metadata_from_calibre(isbn="9781449340377")
        assert result is None

    def test_no_identifier_returns_none(self):
        result = fetch_metadata_from_calibre()
        assert result is None

    def test_abstract_html_stripped(self, mocker):
        opf_with_html = OPF_SAMPLE.replace(
            "Recipes for Python 3 programming.",
            "<p>Recipes for <b>Python 3</b> programming.</p>",
        )
        mock_run = mocker.patch("calibrectl.metadata.subprocess.run")
        mock_run.return_value = MagicMock(returncode=0, stdout=opf_with_html)

        result = fetch_metadata_from_calibre(isbn="9781449340377")
        assert result is not None
        assert "<p>" not in result.get("abstract", "")
        assert "<b>" not in result.get("abstract", "")

    def test_date_with_time_truncated(self, mocker):
        opf_with_time = OPF_SAMPLE.replace(
            "<dc:date>2013-05-10</dc:date>",
            "<dc:date>2013-05-10T00:00:00+00:00</dc:date>",
        )
        mock_run = mocker.patch("calibrectl.metadata.subprocess.run")
        mock_run.return_value = MagicMock(returncode=0, stdout=opf_with_time)

        result = fetch_metadata_from_calibre(isbn="9781449340377")
        assert result is not None
        assert result["pubdate"] == "2013-05-10"
