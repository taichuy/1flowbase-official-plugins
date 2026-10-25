use super::*;

#[test]
fn aes_ecb_matches_nist_known_answer_and_both_provider_key_encodings() {
    // FIPS-197 Appendix C.1; independent of the fixture encryption helper.
    let key = decode_hex(b"000102030405060708090a0b0c0d0e0f").unwrap();
    let ciphertext = decode_hex(b"69c4e0d86a7b0430d8cdb78070b4c55a").unwrap();
    let expected = decode_hex(b"00112233445566778899aabbccddeeff").unwrap();
    assert_eq!(decrypt(ciphertext, &key, false).unwrap(), expected);
    for encoded in [
        STANDARD.encode(&key),
        STANDARD.encode(b"000102030405060708090a0b0c0d0e0f"),
    ] {
        assert_eq!(aes_key(&json!({"aes_key":encoded})).unwrap(), key);
    }
    for invalid in ["not-base64", "YQ==", ""] {
        assert!(aes_key(&json!({"aes_key":invalid})).is_err());
    }
}

#[test]
fn all_media_kinds_and_image_hex_key_are_normalized_without_transcript_shortcut() {
    for (kind, field, mime) in [
        (2, "image_item", "image/jpeg"),
        (3, "voice_item", "audio/silk"),
        (4, "file_item", "application/octet-stream"),
        (5, "video_item", "video/mp4"),
    ] {
        let mut item = json!({"type":kind});
        item[field] = json!({"media":{"encrypt_query_param":"opaque","aes_key":STANDARD.encode(b"0123456789abcdef")},"text":"untrusted provider transcript"});
        if kind == 2 {
            item[field]["aeskey"] = json!("000102030405060708090a0b0c0d0e0f");
        }
        let file = attachment(&item, "provider-id", 1).unwrap().unwrap();
        assert_eq!(file.provider_attachment_id, "provider-id:1");
        assert_eq!(file.media_type, mime);
        assert!(file.size_bytes.is_none());
        file.reference.validate().unwrap();
        if kind == 2 {
            assert_eq!(
                aes_key(&file.reference.download_token["media"]).unwrap(),
                decode_hex(b"000102030405060708090a0b0c0d0e0f").unwrap()
            );
        }
    }
}

#[test]
fn full_url_is_limited_to_exact_https_cdn_download_endpoint() {
    assert!(locator(&json!({"full_url":"https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=a%2Bb"})).is_ok());
    for url in [
        "http://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=x",
        "https://novac2c.cdn.weixin.qq.com:444/c2c/download?encrypted_query_param=x",
        "https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=x&redirect=https://evil.com",
        "https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=x#secret",
        "https://evil.weixin.qq.com/c2c/download?encrypted_query_param=x",
    ] { assert!(locator(&json!({"full_url":url})).is_err(),"{url}"); }
}

#[test]
fn ignored_http_range_streams_prefix_without_a_file_size_cap() {
    let mut buffer = RangeBuffer::new(160 * 1024 + 3, 160 * 1024 + 19);
    // Network chunks preceding the requested offset never accumulate in memory.
    for _ in 0..10 {
        assert!(!buffer.push(&[0u8; 16 * 1024]).unwrap());
        assert!(buffer.data.is_empty());
    }
    let final_network_chunk: Vec<u8> = (0..64).collect();
    assert!(buffer.push(&final_network_chunk).unwrap());
    let (bytes, eof) = buffer.finish().unwrap();
    assert_eq!(bytes, final_network_chunk[3..20]);
    assert!(!eof);
    let mut final_range = RangeBuffer::new(3, 100);
    assert!(!final_range.push(b"prefix and end").unwrap());
    let (bytes, eof) = final_range.finish().unwrap();
    assert_eq!(bytes, b"fix and end");
    assert!(eof);
}
