"""Pure tests for browser_helpers: which processes count as the session's Chromium (bug ae8849f5)."""

from browser_helpers import browser_argvs

BROWSER = ["/nix/store/x-chromium/bin/chromium", "--remote-debugging-port=0", "--user-data-dir=/tmp/p", "--headless=new"]
UTILITY = ["/proc/self/exe", "--type=utility", "--utility-sub-type=network.mojom.NetworkService", "--user-data-dir=/tmp/p"]
RENDERER = ["/proc/self/exe", "--type=renderer", "--user-data-dir=/tmp/p"]
UNRELATED = ["/bin/sleep", "1"]


def test_only_the_browser_process_counts_not_its_children():
    assert browser_argvs([BROWSER, UTILITY, RENDERER, UNRELATED]) == [BROWSER]


def test_a_child_seen_alone_is_not_a_browser():
    assert browser_argvs([UTILITY]) == []


def test_processes_without_a_user_data_dir_are_ignored():
    assert browser_argvs([UNRELATED]) == []
