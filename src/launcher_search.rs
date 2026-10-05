//! Offline, cached phonetic indexes. This runs in the session HTTP worker, never CEF.
use anyhow::{Context, Result};
use pinyin::{ToPinyin, ToPinyinMulti};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Path,
};
use unicode_normalization::UnicodeNormalization;
use vibrato::{dictionary::Dictionary, Tokenizer};

#[derive(Clone, Serialize)]
pub struct SearchIndex {
    terms: Vec<String>,
}
#[derive(Default)]
pub struct SearchCache {
    tokenizer: Option<Tokenizer>,
    entries: BTreeMap<String, SearchIndex>,
}
fn normalize(s: &str) -> String {
    let decomposed = s
        .nfkc()
        .collect::<String>()
        .nfd()
        .filter(|c| {
            !unicode_normalization::char::is_combining_mark(*c)
                || ['\u{3099}', '\u{309a}'].contains(c)
        })
        .collect::<String>();
    decomposed
        .nfc()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

impl SearchCache {
    pub fn batch(&mut self, path: &Path, names: &[String]) -> Result<serde_json::Value> {
        if self.tokenizer.is_none() {
            let reader = zstd::Decoder::new(
                File::open(path).context("Japanese search dictionary missing")?,
            )?;
            self.tokenizer = Some(Tokenizer::new(Dictionary::read(reader)?).ignore_space(true)?);
        }
        // A bounded cache also avoids keeping every renamed app forever.
        if self.entries.len() + names.len() > 8192 {
            self.entries.clear();
        }
        for name in names {
            if !self.entries.contains_key(name) {
                self.entries
                    .insert(name.clone(), index(name, self.tokenizer.as_ref().unwrap()));
            }
        }
        let result = serde_json::to_value(
            names
                .iter()
                .map(|name| (name, &self.entries[name]))
                .collect::<BTreeMap<_, _>>(),
        )?;
        let bytes: usize = self
            .entries
            .iter()
            .map(|(name, index)| name.len() + index.terms.iter().map(String::len).sum::<usize>())
            .sum();
        if bytes > 8 * 1024 * 1024 {
            self.entries.clear();
        }
        Ok(result)
    }
}
fn index(name: &str, tokenizer: &Tokenizer) -> SearchIndex {
    let name = name.nfkc().collect::<String>();
    let mut terms = BTreeSet::new();
    terms.insert(normalize(&name));
    terms.insert(normalize(
        &name
            .split(|c: char| !c.is_alphanumeric())
            .filter_map(|w| w.chars().next())
            .collect::<String>(),
    ));
    let parts: Vec<String> = name
        .chars()
        .map(|c| {
            c.to_pinyin()
                .map(|p| p.plain().into())
                .unwrap_or_else(|| c.to_string())
        })
        .collect();
    terms.insert(normalize(&parts.concat()));
    terms.insert(normalize(
        &parts
            .iter()
            .filter_map(|p| p.chars().next())
            .collect::<String>(),
    ));
    // Add individual alternate readings without a combinatorial expansion.
    for (i, c) in name.chars().enumerate() {
        if terms.len() >= 64 {
            break;
        }
        if let Some(readings) = c.to_pinyin_multi() {
            for reading in readings.into_iter().take(8) {
                if terms.len() >= 64 {
                    break;
                }
                let mut variant = parts.clone();
                variant[i] = reading.plain().into();
                terms.insert(normalize(&variant.concat()));
                terms.insert(normalize(
                    &variant
                        .iter()
                        .filter_map(|p| p.chars().next())
                        .collect::<String>(),
                ));
            }
        }
    }
    let mut worker = tokenizer.new_worker();
    worker.reset_sentence(&name);
    worker.tokenize();
    let mut roman = String::new();
    let mut initials = String::new();
    for i in 0..worker.num_tokens() {
        let token = worker.token(i);
        let reading = token
            .feature()
            .split(',')
            .nth(7)
            .filter(|r| *r != "*")
            .unwrap_or(token.surface());
        let (full, short) = romanize(reading);
        roman.push_str(&full);
        initials.push_str(&short);
    }
    terms.insert(normalize(&roman));
    terms.insert(normalize(&initials));
    let (full, short) = romanize(&name);
    terms.insert(normalize(&full));
    terms.insert(normalize(&short));
    terms.remove("");
    SearchIndex {
        terms: terms.into_iter().collect(),
    }
}
// Hepburn-style syllables, retaining explicit vowels (トウキョウ -> toukyou).
fn romanize(value: &str) -> (String, String) {
    let chars: Vec<char> = value
        .nfkc()
        .map(|c| {
            if ('ぁ'..='ゖ').contains(&c) {
                char::from_u32(c as u32 + 0x60).unwrap()
            } else {
                c
            }
        })
        .collect();
    let mut full = String::new();
    let mut short = String::new();
    let mut i = 0;
    let mut geminate = false;
    let mut in_latin = false;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c == 'ッ' {
            geminate = true;
            continue;
        }
        if c == 'ー' {
            if let Some(v) = full.chars().rev().find(|c| "aeiou".contains(*c)) {
                full.push(v);
            }
            continue;
        }
        let syllable = match c {
            'ア' | 'ァ' => "a",
            'イ' | 'ィ' => "i",
            'ウ' | 'ゥ' => "u",
            'エ' | 'ェ' => "e",
            'オ' | 'ォ' => "o",
            'カ' => "ka",
            'キ' => "ki",
            'ク' => "ku",
            'ケ' => "ke",
            'コ' => "ko",
            'サ' => "sa",
            'シ' => "shi",
            'ス' => "su",
            'セ' => "se",
            'ソ' => "so",
            'タ' => "ta",
            'チ' => "chi",
            'ツ' => "tsu",
            'テ' => "te",
            'ト' => "to",
            'ナ' => "na",
            'ニ' => "ni",
            'ヌ' => "nu",
            'ネ' => "ne",
            'ノ' => "no",
            'ハ' => "ha",
            'ヒ' => "hi",
            'フ' => "fu",
            'ヘ' => "he",
            'ホ' => "ho",
            'マ' => "ma",
            'ミ' => "mi",
            'ム' => "mu",
            'メ' => "me",
            'モ' => "mo",
            'ヤ' | 'ャ' => "ya",
            'ユ' | 'ュ' => "yu",
            'ヨ' | 'ョ' => "yo",
            'ラ' => "ra",
            'リ' => "ri",
            'ル' => "ru",
            'レ' => "re",
            'ロ' => "ro",
            'ワ' | 'ヮ' => "wa",
            'ヰ' => "wi",
            'ヱ' => "we",
            'ヲ' => "wo",
            'ン' => "n",
            'ガ' => "ga",
            'ギ' => "gi",
            'グ' => "gu",
            'ゲ' => "ge",
            'ゴ' => "go",
            'ザ' => "za",
            'ジ' => "ji",
            'ズ' => "zu",
            'ゼ' => "ze",
            'ゾ' => "zo",
            'ダ' => "da",
            'ヂ' => "ji",
            'ヅ' => "zu",
            'デ' => "de",
            'ド' => "do",
            'バ' => "ba",
            'ビ' => "bi",
            'ブ' => "bu",
            'ベ' => "be",
            'ボ' => "bo",
            'パ' => "pa",
            'ピ' => "pi",
            'プ' => "pu",
            'ペ' => "pe",
            'ポ' => "po",
            'ヴ' => "vu",
            _ => "",
        };
        if syllable.is_empty() {
            full.push(c);
            if c.is_alphanumeric() {
                if !in_latin {
                    short.push(c);
                }
                in_latin = true;
            } else {
                in_latin = false;
            }
            geminate = false;
            continue;
        }
        in_latin = false;
        let mut syllable = syllable.to_owned();
        if i < chars.len() {
            let small = chars[i];
            if "ャュョ".contains(small) && syllable.ends_with('i') {
                let base = match syllable.as_str() {
                    "shi" => "sh",
                    "chi" => "ch",
                    "ji" => "j",
                    _ => &syllable[..syllable.len() - 1],
                };
                syllable = format!(
                    "{}{}{}",
                    base,
                    if ["sh", "ch", "j"].contains(&base) {
                        ""
                    } else {
                        "y"
                    },
                    match small {
                        'ャ' => "a",
                        'ュ' => "u",
                        _ => "o",
                    }
                );
                i += 1;
            } else if "ァィゥェォ".contains(small)
                && !["a", "i", "u", "e", "o"].contains(&syllable.as_str())
            {
                let base = if syllable == "fu" {
                    "f"
                } else if syllable == "vu" {
                    "v"
                } else {
                    &syllable[..syllable.len() - 1]
                };
                syllable = format!(
                    "{}{}",
                    base,
                    match small {
                        'ァ' => "a",
                        'ィ' => "i",
                        'ゥ' => "u",
                        'ェ' => "e",
                        _ => "o",
                    }
                );
                i += 1;
            }
        }
        if geminate {
            if let Some(first) = syllable.chars().next().filter(|c| !"aeioun".contains(*c)) {
                full.push(first);
            }
            geminate = false;
        }
        short.push(syllable.chars().next().unwrap());
        full.push_str(&syllable);
    }
    (full, short)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_multilingual_index_and_cache() {
        let mut cache = SearchCache::default();
        let names = vec![
            "透视调色",
            "さくら",
            "東京",
            "Half-Life: Alyx",
            "Ｔｅｒｍｉｘ",
            "ゲーム",
            "キャット",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let v = cache
            .batch(Path::new("assets/search/ipadic.dic.zst"), &names)
            .unwrap();
        println!("Launcher index cold: {:?}", started.elapsed());
        for (name, needle) in [
            ("透视调色", "toushi"),
            ("透视调色", "tiaose"),
            ("透视调色", "tsts"),
            ("さくら", "sakura"),
            ("さくら", "skr"),
            ("東京", "toukyou"),
            ("Half-Life: Alyx", "hla"),
            ("Ｔｅｒｍｉｘ", "termix"),
            ("ゲーム", "geemu"),
            ("キャット", "kyatto"),
        ] {
            assert!(
                v[name]["terms"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t.as_str().unwrap().contains(needle)),
                "{name}: {needle}, {}",
                v[name]
            );
        }
        let cached = std::time::Instant::now();
        assert_eq!(v, cache.batch(Path::new("does-not-exist"), &names).unwrap());
        println!("Launcher index cached: {:?}", cached.elapsed());
    }
    #[test]
    fn kana_and_normalization() {
        assert_eq!(romanize("さくら"), ("sakura".into(), "skr".into()));
        assert_eq!(romanize("トウキョウ").0, "toukyou");
        assert_eq!(normalize("ＴＯＵ shí-"), "toushi");
        assert_ne!(normalize("ガ"), normalize("カ"));
        assert_eq!(romanize("Ｍｙ アプリ").0, "My apuri");
    }
}
