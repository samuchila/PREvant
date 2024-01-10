/*-
 * ========================LICENSE_START=================================
 * PREvant REST API
 * %%
 * Copyright (C) 2018 - 2019 aixigo AG
 * %%
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 * =========================LICENSE_END==================================
 */
use chrono::{DateTime, FixedOffset, Utc};
use std::convert::From;

#[derive(Serialize)]
pub struct LogChunk {
    since: DateTime<FixedOffset>,
    until: DateTime<FixedOffset>,
    log_lines: String,
}

impl LogChunk {
    #[cfg(test)]
    pub fn since(&self) -> &DateTime<FixedOffset> {
        &self.since
    }

    pub fn until(&self) -> &DateTime<FixedOffset> {
        &self.until
    }

    pub fn log_lines(&self) -> &String {
        &self.log_lines
    }
}

impl From<Vec<(DateTime<FixedOffset>, String)>> for LogChunk {
    fn from(logs: Vec<(DateTime<FixedOffset>, String)>) -> Self {
        let since = DateTime::<Utc>::MAX_UTC.fixed_offset();
        let until = DateTime::<Utc>::MIN_UTC.fixed_offset();

        let chunk = LogChunk {
            since,
            until,
            log_lines: String::from(""),
        };

        logs.iter().fold(chunk, |mut chunk, log_line| {
            chunk.since = if chunk.since > log_line.0 {
                log_line.0
            } else {
                chunk.since
            };
            chunk.until = if chunk.until < log_line.0 {
                log_line.0
            } else {
                chunk.until
            };

            chunk.log_lines.push_str(&log_line.1);

            chunk
        })
    }
}
