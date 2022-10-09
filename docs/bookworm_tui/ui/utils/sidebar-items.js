window.SIDEBAR_ITEMS = {"enum":[["AppView",""],["ApplicationTask",""]],"fn":[["char_chunks_to_styled_text","Takes the CharChunk and styles it with the provided styling rules."],["copy_from_clipboard",""],["cut_word_to_fit","Takes `word`, and cuts excess letters to ensure that it fits within `max_width` visible characters. If `word` is too long, it will be truncated and have ‘…’ appended to indicate that it has been truncated (if `max_width` is at least 3, otherwise, letters will simply be cut). It will then be returned as a `ListItem`."],["paste_into_clipboard",""],["run_command",""],["split_chunk_into_columns","Splits `chunk` into `num_cols` columns with widths differing by no more than one, and adding up to the width of `chunk`, except when `num_cols` is 0. If called with sequentially increasing or decreasing values, chunk sizes will never decrease or increase, respectively."],["to_tui",""]],"struct":[["StyleRules",""]],"trait":[["TuiStyle",""]]};