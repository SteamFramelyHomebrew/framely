# Offline launcher search data

`ipadic.dic.zst` is the IPADic 2.7.0 dictionary compiled for Vibrato 0.5.0.
Vibrato is pinned to the matching reader format; upgrade the reader and dictionary together.

Source: https://github.com/daac-tools/vibrato/releases/download/v0.5.0/ipadic-mecab-2_7_0.tar.xz

Source archive SHA-256: `4764f983b7c3a9e1cb6a5ee945e00558efd812980e0dad61224f63ee3b0475d9`

Dictionary SHA-256: `82a6da70bb4a17be70f20ff44f650f9ad1d2b0b4fcb2f39c17fc797f92d0ab75`

Chinese pinyin uses rust-pinyin 0.11.0 and pinyin-data 0.15.0 (MIT); see `pinyin-LICENSE`.
Vibrato license notices are included as `vibrato-LICENSE-*`.

Keep `COPYING` and `NOTICE` with the dictionary. Release packaging copies this directory to
`share/search`, includes it in core updates and offline packages, and records its checksum.
The dictionary is loaded lazily by the session's search worker. No remote service is used.

中文：此目录包含 Vibrato 0.5.0 格式的 IPADic 2.7.0 离线日语读音词典。读入器与词典必须一起升级。
发布包复制到 `share/search`，包含于本体更新包和完整离线包，并纳入校验。
请保留 `COPYING` 和 `NOTICE`。词典由会话搜索线程按需加载，不使用远程服务。
