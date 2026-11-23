//! **Folding for the picker** — case, and the desk's own small diacritic table.
//!
//! This is **this table**, not Unicode normalisation. There is no combining-mark stripping, no locale
//! tailoring and no canonically-equivalent fallback: a Latin letter folds because it appears in the table
//! below, and a letter that does not appear folds only by case. That is deliberate. Shipping a normaliser
//! would be a dependency for a search over a few hundred hull names, and a normaliser whose behaviour
//! changes with a Unicode update is a search whose ranking changes underneath a saved query.
//!
//! Uppercase is lowercased first (`É` → `é` → `e`), then the table applies to the lowercase character.
//!
//! | folds to | characters |
//! | --- | --- |
//! | `a` | à á â ã ä å ā ă ą |
//! | `ae` | æ |
//! | `c` | ç ć ĉ ċ č |
//! | `d` | ď đ ð |
//! | `e` | è é ê ë ē ĕ ė ę ě |
//! | `g` | ğ ĝ ġ ģ |
//! | `h` | ĥ ħ |
//! | `i` | ì í î ï ī ĭ į ı |
//! | `j` | ĵ |
//! | `k` | ķ |
//! | `l` | ĺ ļ ľ ł |
//! | `n` | ñ ń ņ ň |
//! | `o` | ò ó ô õ ö ø ō ŏ ő |
//! | `oe` | œ |
//! | `r` | ŕ ŗ ř |
//! | `s` | ś ŝ ş š |
//! | `ss` | ß |
//! | `t` | ţ ť ŧ |
//! | `th` | þ |
//! | `u` | ù ú û ü ū ŭ ů ű ų |
//! | `w` | ŵ |
//! | `y` | ý ÿ ŷ |
//! | `z` | ź ż ž |

/// The folded form a query and a field are both compared in.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        for lower in character.to_lowercase() {
            push_folded(&mut out, lower);
        }
    }
    out
}

fn push_folded(out: &mut String, character: char) {
    let folded = match character {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ğ' | 'ĝ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => {
            out.push(character);
            return;
        }
    };
    out.push_str(folded);
}
