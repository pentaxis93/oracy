use oracy_backend::audio_hash::compose_audio_content_hash_hex;

#[test]
fn composed_audio_content_hash_uses_raw_chunk_digest_bytes_in_order() {
    let chunk_hashes = [
        "b18efa847f7a3fa48fe0aafd4a6250aa5129740e05126859377af20cedafdeee",
        "2725aea2d2dea736fbe38e41ecb518f6098cb68ac77362c5e34faafb356567c5",
        "b6f3c15844fe12f966ad90db59da8332a9a6d9dfd198ac83949be2045ec6dc1e",
    ];

    let composed = compose_audio_content_hash_hex(chunk_hashes).expect("compose hash");

    assert_eq!(
        composed,
        "5bdae50ac99ab32dd48fbc23bf45c6415fd318e770a9b846c86b7cb7c1087a93"
    );
}

#[test]
fn composed_audio_content_hash_rejects_invalid_chunk_digest_shapes() {
    for invalid in [
        "b18efa847f7a3fa48fe0aafd4a6250aa5129740e05126859377af20cedafdee",
        "b18efa847f7a3fa48fe0aafd4a6250aa5129740e05126859377af20cedafdeeg",
        "B18EFA847F7A3FA48FE0AAFD4A6250AA5129740E05126859377AF20CEDAFDEEE",
    ] {
        assert!(
            compose_audio_content_hash_hex([invalid]).is_err(),
            "invalid digest should be rejected: {invalid}"
        );
    }
}
