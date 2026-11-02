mod common;

use common::parity::load_fixture_value;
use tandem::coordination::{Claim, Query, Session, decode_query_fields, encode_query_fields};

#[test]
fn test_coordination_parity_fixture() {
    let fixture = load_fixture_value("coordination");

    let expected_encoded = fixture["encode_query_fields"].as_str().unwrap();
    let decoded_fields = decode_query_fields(expected_encoded).unwrap();
    let re_encoded = encode_query_fields(&decoded_fields);
    let re_decoded = decode_query_fields(&re_encoded).unwrap();
    assert_eq!(re_decoded, decoded_fields);
    assert_eq!(
        &decoded_fields,
        fixture["decode_query_fields"].as_object().unwrap()
    );

    let claim_raw = fixture["claim_from_dict"].as_object().unwrap();
    let claim = Claim::from_dict(claim_raw).unwrap();
    assert_eq!(claim.phase, "p-001");
    assert_eq!(claim.task, "t-002");
    assert_eq!(claim.session, "s-1");
    assert_eq!(claim.paths, vec!["src/lib.rs"]);

    let session_raw = fixture["session_from_dict"].as_object().unwrap();
    let session = Session::from_dict(session_raw).unwrap();
    assert_eq!(session.id, "s-1");
    assert_eq!(session.cli, "claude");
    assert_eq!(session.role, "implementer");

    let query_raw = fixture["query_from_dict"].as_object().unwrap();
    let query = Query::from_dict(query_raw).unwrap();
    assert_eq!(query.id, "q-1");
    assert_eq!(query.status, "open");
    assert_eq!(query.asked_by, "implementer-1");
}
