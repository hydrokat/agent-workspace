from tandem.tmux.layout import conf_text


def test_conf_enables_mouse_and_disables_prefix() -> None:
    text = conf_text()
    assert "mouse on" in text and "prefix None" in text and "remain-on-exit on" in text


def test_conf_has_no_command_execution() -> None:
    text = conf_text()
    for word in ("run-shell", "send-keys", "if-shell", "source-file"):
        assert word not in text
