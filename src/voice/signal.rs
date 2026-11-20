//! The messages the two ends of a call exchange to find each other, in the form a browser's
//! `RTCPeerConnection` uses: Bingle relays them without reading them (its `docs/voice.md` and the
//! schema's `$defs/SignalData`), so a page on the other end must understand what this sends.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One message for the other end of a call, or from it.
#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    /// A session description offering the call, as SDP.
    Offer(String),
    /// The answer to an offer, as SDP.
    Answer(String),
    /// One network address to try, trickled as it is found. Nothing marks the last.
    Ice(Candidate),
    /// The side that does not offer asks the other to offer again.
    Retry,
}

/// `RTCIceCandidate.toJSON()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub candidate: String,
    #[serde(rename = "sdpMid", default)]
    pub sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex", default)]
    pub sdp_mline_index: Option<u16>,
    #[serde(rename = "usernameFragment", default)]
    pub username_fragment: Option<String>,
}

#[derive(Deserialize)]
struct Description {
    #[serde(rename = "type")]
    kind: String,
    sdp: String,
}

impl Signal {
    /// The message's `kind`: `offer`, `answer`, `ice` or `retry`.
    pub fn kind(&self) -> &'static str {
        match self {
            Signal::Offer(_) => "offer",
            Signal::Answer(_) => "answer",
            Signal::Ice(_) => "ice",
            Signal::Retry => "retry",
        }
    }

    /// The message's `data`, as a browser would send it.
    pub fn data(&self) -> Value {
        match self {
            Signal::Offer(sdp) => json!({"type": "offer", "sdp": sdp}),
            Signal::Answer(sdp) => json!({"type": "answer", "sdp": sdp}),
            Signal::Ice(c) => json!({
                "candidate": c.candidate,
                "sdpMid": c.sdp_mid,
                "sdpMLineIndex": c.sdp_mline_index,
                "usernameFragment": c.username_fragment,
            }),
            Signal::Retry => json!({}),
        }
    }

    /// Reads a message from the other end, `kind` and `data` as they came.
    pub fn read(kind: &str, data: &Value) -> Result<Signal, String> {
        let description = |expected: &str| -> Result<String, String> {
            let d = Description::deserialize(data).map_err(|e| format!("{kind}: {e}"))?;
            if d.kind != expected {
                return Err(format!("{kind}: a description of type {:?}", d.kind));
            }
            Ok(d.sdp)
        };
        match kind {
            "offer" => description("offer").map(Signal::Offer),
            "answer" => description("answer").map(Signal::Answer),
            "ice" => Candidate::deserialize(data)
                .map(Signal::Ice)
                .map_err(|e| format!("ice: {e}")),
            "retry" => Ok(Signal::Retry),
            other => Err(format!("a signal of unknown kind {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_what_a_browser_sends() {
        let offer = Signal::Offer("v=0\r\n".into());
        assert_eq!(offer.kind(), "offer");
        assert_eq!(offer.data(), json!({"type": "offer", "sdp": "v=0\r\n"}));
        assert_eq!(Signal::read("offer", &offer.data()), Ok(offer));
        let answer = Signal::Answer("v=0\r\n".into());
        assert_eq!(Signal::read("answer", &answer.data()), Ok(answer));
    }

    #[test]
    fn a_description_of_the_wrong_type_is_refused() {
        let data = json!({"type": "answer", "sdp": "v=0"});
        assert!(Signal::read("offer", &data).is_err());
        assert!(Signal::read("offer", &json!({"sdp": "v=0"})).is_err());
    }

    #[test]
    fn candidates_read_a_browsers_json() {
        // What Chrome's RTCIceCandidate.toJSON() gives, and the smallest the schema allows.
        let chrome = json!({
            "candidate": "candidate:1 1 udp 2122260223 192.0.2.7 54321 typ host generation 0",
            "sdpMid": "0",
            "sdpMLineIndex": 0,
            "usernameFragment": "abcd"
        });
        let Ok(Signal::Ice(c)) = Signal::read("ice", &chrome) else {
            panic!("not read");
        };
        assert_eq!(c.sdp_mid.as_deref(), Some("0"));
        assert_eq!(c.sdp_mline_index, Some(0));
        assert_eq!(Signal::Ice(c).data(), chrome);
        let bare = json!({"candidate": "candidate:1 1 udp 1 192.0.2.7 1 typ host"});
        let Ok(Signal::Ice(c)) = Signal::read("ice", &bare) else {
            panic!("not read");
        };
        assert_eq!(c.sdp_mid, None);
        let nulls = json!({"candidate": "", "sdpMid": null, "sdpMLineIndex": null});
        assert!(Signal::read("ice", &nulls).is_ok());
    }

    #[test]
    fn retry_is_an_empty_object() {
        assert_eq!(Signal::Retry.data(), json!({}));
        assert_eq!(Signal::read("retry", &json!({})), Ok(Signal::Retry));
        assert!(Signal::read("hello", &json!({})).is_err());
    }
}
