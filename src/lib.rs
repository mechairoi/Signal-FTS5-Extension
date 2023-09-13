//
// Copyright 2023 Signal Messenger, LLC.
// SPDX-License-Identifier: AGPL-3.0-only
//

extern crate alloc;

mod common;
#[cfg(feature = "extension")]
mod extension;

pub use crate::common::*;
use libc::{c_char, c_int, c_uchar, c_void};
use unicode_normalization::UnicodeNormalization;

#[no_mangle]
pub extern "C" fn signal_fts5_tokenize(
    tokenizer: *mut Fts5Tokenizer,
    p_ctx: *mut c_void,
    _flags: c_int,
    p_text: *const c_char,
    n_text: c_int,
    x_token: TokenFunction,
) -> c_int {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || match signal_fts5_tokenize_internal(tokenizer, p_ctx, p_text, n_text, x_token) {
            Ok(()) => SQLITE_OK,
            Err(code) => code,
        },
    ))
    .unwrap_or(SQLITE_INTERNAL)
}

fn signal_fts5_tokenize_internal(
    tokenizer: *mut Fts5Tokenizer,
    p_ctx: *mut c_void,
    p_text: *const c_char,
    n_text: c_int,
    x_token: TokenFunction,
) -> Result<(), c_int> {
    let slice = unsafe { core::slice::from_raw_parts(p_text as *const c_uchar, n_text as usize) };

    // Map errors to SQLITE_OK because failing here means that the database
    // wouldn't accessible.
    let input = core::str::from_utf8(slice).map_err(|_| SQLITE_OK)?;

    let mut normalized = String::with_capacity(1024);

    match unsafe { (*tokenizer).analyzer.analyze(input) } {
        Ok(tokens) => {
            for token in tokens {
                normalize_into(token.text.as_str(), &mut normalized);

                let rc = x_token(
                    p_ctx,
                    0,
                    normalized.as_bytes().as_ptr() as *const c_char,
                    normalized.len() as c_int,
                    token.byte_start as c_int,
                    token.byte_end as c_int,
                );
                if rc != SQLITE_OK {
                    return Err(rc);
                }
            }
        }
        Err(_) => {
            return Err(SQLITE_INTERNAL);
        }
    }

    return Ok(());
}

fn is_diacritic(x: char) -> bool {
    '\u{0300}' <= x && x <= '\u{036f}'
}

fn normalize_into(segment: &str, buf: &mut String) {
    buf.clear();

    for x in segment.nfd() {
        if is_diacritic(x) {
            continue;
        }
        if x.is_ascii() {
            buf.push(x.to_ascii_lowercase());
        } else {
            buf.extend(x.to_lowercase());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lindera_analyzer::analyzer::Analyzer;

    #[test]
    fn it_normalizes_segment() {
        let mut buf = String::new();
        normalize_into("DïācRîtįcs", &mut buf);
        assert_eq!(buf, "diacritics");
    }

    extern "C" fn token_callback(
        ctx: *mut c_void,
        flags: c_int,
        token: *const c_char,
        token_len: c_int,
        start: c_int,
        end: c_int,
    ) -> c_int {
        assert_eq!(flags, 0);

        let tokens_ptr = ctx as *mut _ as *mut Vec<(String, c_int, c_int)>;
        let tokens = unsafe { tokens_ptr.as_mut() }.expect("tokens pointer");
        let slice =
            unsafe { core::slice::from_raw_parts(token as *const c_uchar, token_len as usize) };
        let token = String::from_utf8(slice.to_vec()).expect("Expected utf-8 token");

        tokens.push((token, start, end));

        return SQLITE_OK;
    }

    #[test]
    fn it_emits_segments() {
        let input = "東京都は雨が降りそうです";
        let mut tokens: Vec<(String, c_int, c_int)> = vec![];

        let mut tokenizer = Fts5Tokenizer {
            analyzer: Analyzer::from_slice(include_bytes!("../resources/lindera_ipadic_conf.json"))
                .unwrap(),
        };
        signal_fts5_tokenize_internal(
            &mut tokenizer,
            &mut tokens as *mut _ as *mut c_void,
            input.as_bytes().as_ptr() as *const c_char,
            input.len() as i32,
            token_callback,
        )
        .expect("tokenize internal should not fail");

        assert_eq!(
            tokens,
            [
                ("東京", 0, 6),
                ("都", 6, 9),
                ("雨", 12, 15),
                ("降りる", 18, 24),
                ("そう", 24, 30)
            ]
            .map(|(s, start, end)| (s.to_owned(), start, end))
        );
    }

    #[test]
    fn it_ignores_invalid_utf8() {
        let input = b"\xc3\x28";
        let mut tokens: Vec<(String, c_int, c_int)> = vec![];

        let mut tokenizer = Fts5Tokenizer {
            analyzer: Analyzer::from_slice(include_bytes!("../resources/lindera_ipadic_conf.json"))
                .unwrap(),
        };
        assert_eq!(
            signal_fts5_tokenize_internal(
                &mut tokenizer,
                &mut tokens as *mut _ as *mut c_void,
                input.as_ptr() as *const c_char,
                input.len() as i32,
                token_callback,
            )
            .expect_err("tokenize internal should not fail"),
            SQLITE_OK
        );

        assert_eq!(tokens, []);
    }
}
