use crate::{PluginError, Result};
use encoding_rs::{Encoding, UTF_8};

pub(super) fn srt_to_vtt(bytes: &[u8], content_type: &str) -> Result<Vec<u8>> {
    let invalid = || PluginError::new("UPSTREAM_CHANGED", "SRT 字幕编码或时间轴无效");
    let (encoding, skip) = if let Some(bom) = Encoding::for_bom(bytes) {
        bom
    } else {
        let charset = content_type.split(';').skip(1).find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case("charset")
                .then(|| value.trim().trim_matches('"'))
        });
        (
            match charset {
                Some(label) => Encoding::for_label(label.as_bytes()).ok_or_else(invalid)?,
                None => UTF_8,
            },
            0,
        )
    };
    let text = encoding
        .decode_without_bom_handling_and_without_replacement(&bytes[skip..])
        .ok_or_else(invalid)?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = String::from("WEBVTT\n\n");
    let mut cues = 0;
    for block in normalized.split("\n\n").filter(|b| !b.trim().is_empty()) {
        let mut lines = block.trim_matches('\n').lines();
        let first = lines.next().ok_or_else(invalid)?.trim();
        let timing = if first.bytes().all(|b| b.is_ascii_digit()) && !first.is_empty() {
            lines.next().ok_or_else(invalid)?
        } else {
            first
        };
        let (start, end) = timing.split_once("-->").ok_or_else(invalid)?;
        let parse = |value: &str| -> Result<(u64, String)> {
            let value = value.trim();
            let parts: Vec<_> = value.split([':', ',']).collect();
            if parts.len() != 4
                || parts[0].len() < 2
                || parts[1].len() != 2
                || parts[2].len() != 2
                || parts[3].len() != 3
                || parts
                    .iter()
                    .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(invalid());
            }
            let numbers: Vec<u64> = parts
                .iter()
                .map(|p| p.parse().map_err(|_| invalid()))
                .collect::<Result<_>>()?;
            if numbers[1] >= 60 || numbers[2] >= 60 {
                return Err(invalid());
            }
            let millis = numbers[0]
                .checked_mul(3_600_000)
                .and_then(|n| n.checked_add(numbers[1] * 60_000 + numbers[2] * 1000 + numbers[3]))
                .ok_or_else(invalid)?;
            Ok((millis, value.replace(',', ".")))
        };
        let (start_ms, start) = parse(start)?;
        let (end_ms, end) = parse(end)?;
        if end_ms <= start_ms {
            return Err(invalid());
        }
        let payload = lines.collect::<Vec<_>>().join("\n");
        if payload.is_empty() {
            return Err(invalid());
        }
        output.push_str(&format!("{start} --> {end}\n{payload}\n\n"));
        if output.len() > super::MAX_RESOURCE {
            return Err(PluginError::new(
                "UPSTREAM_CHANGED",
                "转换后的字幕超过缓冲预算",
            ));
        }
        cues += 1;
    }
    if cues == 0 {
        return Err(invalid());
    }
    Ok(output.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SRT: &str = "1\r\n00:00:01,200 --> 00:00:03,400\r\n你好\r\n<b>world</b>\r\n\r\n2\r\n01:02:03,000 --> 01:02:04,000\r\n字幕\r\n";
    #[test]
    fn converts_encoding_and_timeline() {
        let expected = "WEBVTT\n\n00:00:01.200 --> 00:00:03.400\n你好\n<b>world</b>\n\n01:02:03.000 --> 01:02:04.000\n字幕\n\n";
        assert_eq!(
            srt_to_vtt(SRT.as_bytes(), "application/octet-stream").unwrap(),
            expected.as_bytes()
        );
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(SRT.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(
            srt_to_vtt(&utf16, "application/octet-stream").unwrap(),
            expected.as_bytes()
        );
        let (gbk, _, _) = encoding_rs::GBK.encode(SRT);
        assert_eq!(
            srt_to_vtt(&gbk, "text/plain; charset=gbk").unwrap(),
            expected.as_bytes()
        );
    }
    #[test]
    fn rejects_bad_subtitles() {
        for value in [
            "",
            "<html>error</html>",
            "1\n00:70:00,000 --> 00:71:00,000\nx",
            "1\n00:00:02,000 --> 00:00:01,000\nx",
        ] {
            assert!(srt_to_vtt(value.as_bytes(), "application/octet-stream").is_err());
        }
        assert!(srt_to_vtt(&[0xff], "text/plain").is_err());
    }
}
