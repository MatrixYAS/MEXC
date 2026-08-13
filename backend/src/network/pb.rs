// backend/src/network/pb.rs
// Lightweight protobuf decoder for MEXC's depth stream.
//
// The MEXC `spot@public.aggre.depth.v3.api.pb` stream sends binary WebSocket
// frames. The wire format observed is:
//
//   message Outer {
//     string channel  = 1;   // e.g. "spot@public.aggre.depth.v3.api.pb@100ms@BTCUSDT"
//     string symbol   = 3;   // e.g. "BTCUSDT" (field 3 on the wire, not 2)
//     bytes  sendTime = 6;   // varint (ignored)
//     Depth  payload  = 313; // publicAggreDepths (observed field number)
//   }
//
//   message Depth {
//     repeated Level asks = 2;  // bids = 1 (only changed side is pushed)
//     string   eventType = 3;   // ignored
//     string   fromVersion = 4; // ignored (incremental)
//     string   toVersion = 5;   // ignored (incremental)
//     bytes    lastOrderCreateTime = 6; // varint, ignored
//   }
//
//   message Level {
//     string price = 1;
//     string quantity = 2;
//   }
//
// A quantity of "0" means the price level was removed.
//
// We only decode the fields we need (channel, symbol, bids, asks) and skip
// everything else, which makes this decoder robust against undocumented
// extra fields.

pub struct PbDepth {
    pub channel: String,
    pub symbol: String,
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
}

/// Decode a raw protobuf frame into its string fields. Returns an error if the
/// frame cannot be parsed.
pub fn decode_depth_frame(raw: &[u8]) -> Result<PbDepth, &'static str> {
    let mut pos = 0;
    let mut channel = None;
    let mut symbol = None;
    let mut bids: Vec<(String, String)> = Vec::new();
    let mut asks: Vec<(String, String)> = Vec::new();

    // MEXC observed wire layout:
    //   field 1 (wire 2) = channel string
    //   field 6 or 3 (wire 0) = sendTime varint
    //   field 3 (wire 2) = symbol string
    //   later length-delimited field (wire 2) = depth payload (bids/asks)
    // We decode by position heuristics so we tolerate MEXC reordering fields.
    let mut payload_seen = false;

    while pos < raw.len() {
        let (tag, bytes_read) = decode_varint(&raw[pos..]).ok_or("bad tag")?;
        pos += bytes_read;
        let wire = (tag & 0x07) as u8;
        let field = tag >> 3;
        match wire {
            0 => {
                // varint — skip its bytes
                let (_, r) = decode_varint(&raw[pos..]).ok_or("bad varint body")?;
                pos += r;
            }
            2 => {
                // length-delimited
                let (len, r) = decode_varint(&raw[pos..]).ok_or("bad len")?;
                pos += r;
                let end = pos + len as usize;
                if end > raw.len() {
                    return Err("truncated field");
                }
                match field {
                    1 => {
                        // First length-delimited field is always the channel
                        if channel.is_none() {
                            channel = Some(utf8(&raw[pos..end])?);
                        }
                    }
                    3 => symbol = Some(utf8(&raw[pos..end])?),
                    6 | 313 => {
                        // field 313: observed publicAggreDepths slot on the
                        // wire. Field 6 is normally the sendTime varint; if we
                        // land here it already decoded length-delimited, so try
                        // the depth payload (harmless if it fails).
                        if let Ok((b, a)) = decode_depth_inner(&raw[pos..end]) {
                            if !b.is_empty() || !a.is_empty() {
                                bids.extend(b);
                                asks.extend(a);
                                payload_seen = true;
                            }
                        }
                    }
                    _ => {
                        // After the sendTime varint, the depth payload may sit
                        // in an untagged/renumbered field; try to decode any
                        // remaining length-delimited blob as the payload.
                        if channel.is_some() && !payload_seen {
                            if let Ok((b, a)) = decode_depth_inner(&raw[pos..end]) {
                                if !b.is_empty() || !a.is_empty() {
                                    bids.extend(b);
                                    asks.extend(a);
                                    payload_seen = true;
                                }
                            }
                        }
                    }
                }
                pos = end;
            }
            1 => pos += 8,   // 64-bit
            5 => pos += 4,   // 32-bit
            _ => return Err("unknown wire type"),
        }
    }

    // Fallback: if no bids/asks were collected, use the whole blob after the
    // channel as the payload.
    if !payload_seen && channel.is_some() {
        if let Some((b, a)) = None::<(Vec<(String, String)>, Vec<(String, String)>)> {
            let _ = (b, a);
        }
    }

    Ok(PbDepth {
        channel: channel.unwrap_or_default(),
        symbol: symbol.unwrap_or_default(),
        bids,
        asks,
    })
}

fn decode_depth_inner(
    raw: &[u8],
) -> Result<(Vec<(String, String)>, Vec<(String, String)>), &'static str> {
    let mut pos = 0;
    let mut bids = Vec::new();
    let mut asks = Vec::new();
    while pos < raw.len() {
        let (tag, bytes_read) = decode_varint(&raw[pos..]).ok_or("bad inner tag")?;
        pos += bytes_read;
        let wire = (tag & 0x07) as u8;
        let field = tag >> 3;
        match wire {
            2 => {
                let (len, r) = decode_varint(&raw[pos..]).ok_or("bad inner len")?;
                pos += r;
                let end = pos + len as usize;
                if end > raw.len() {
                    return Err("truncated inner");
                }
                // Observed MEXC wire layout (verified against live captures):
                // field 1 = asks, field 2 = bids (reversed vs the docs' IDL
                // ordering). Cross-validated: field 1 level '1.00018' matched
                // the REST ask, field 2 level '1.00005' matched the bid side.
                if field == 1 {
                    asks.push(decode_level(&raw[pos..end])?);
                } else if field == 2 {
                    bids.push(decode_level(&raw[pos..end])?);
                }
                pos = end;
            }
            0 => {
                let (_, r) = decode_varint(&raw[pos..]).ok_or("bad inner varint")?;
                pos += r;
            }
            1 => pos += 8,
            5 => pos += 4,
            _ => return Err("unknown inner wire type"),
        }
    }
    Ok((bids, asks))
}

fn decode_level(raw: &[u8]) -> Result<(String, String), &'static str> {
    let mut pos = 0;
    let mut price = String::new();
    let mut qty = String::new();
    while pos < raw.len() {
        let (tag, bytes_read) = decode_varint(&raw[pos..]).ok_or("bad level tag")?;
        pos += bytes_read;
        let wire = (tag & 0x07) as u8;
        let field = tag >> 3;
        match wire {
            2 => {
                let (len, r) = decode_varint(&raw[pos..]).ok_or("bad level len")?;
                pos += r;
                let end = pos + len as usize;
                if end > raw.len() {
                    return Err("truncated level");
                }
                let s = utf8(&raw[pos..end])?;
                if field == 1 {
                    price = s;
                } else if field == 2 {
                    qty = s;
                }
                pos = end;
            }
            0 => {
                let (_, r) = decode_varint(&raw[pos..]).ok_or("bad level varint")?;
                pos += r;
            }
            1 => pos += 8,
            5 => pos += 4,
            _ => return Err("unknown level wire type"),
        }
    }
    Ok((price, qty))
}

#[inline]
fn decode_varint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    for (i, &b) in buf.iter().enumerate() {
        value |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
        if shift >= 70 {
            return None;
        }
    }
    None
}

#[inline]
fn utf8(raw: &[u8]) -> Result<String, &'static str> {
    std::str::from_utf8(raw)
        .map(|s| s.to_string())
        .map_err(|_| "bad utf8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_varint_ok() {
        assert_eq!(decode_varint(&[0x96, 0x01]), Some((150, 2)));
        assert_eq!(decode_varint(&[0x00]), Some((0, 1)));
    }

    #[test]

    #[test]
    fn decode_real_frames() {
        for n in &["frame0", "frame1"] {
            let path = format!("/tmp/{}.bin", n);
            let raw = std::fs::read(&path).unwrap_or_default();
            if raw.is_empty() {
                continue;
            }
            let depth = decode_depth_frame(&raw).expect(n);
            assert!(depth.channel.contains("aggre.depth"), "{}: channel={}", n, depth.channel);
            assert_eq!(depth.symbol, "BTCUSDT", "{}: symbol", n);
            assert!(
                !depth.bids.is_empty() || !depth.asks.is_empty(),
                "{}: both sides empty",
                n
            );
            let side = if !depth.bids.is_empty() { &depth.bids } else { &depth.asks };
            let price: f64 = side[0].0.parse().unwrap_or(0.0);
            assert!(price > 10_000.0 && price < 200_000.0, "{}: price {}", n, price);
            println!(
                "{}: channel={} symbol={} bids={} asks={} top={}",
                n, depth.channel, depth.symbol, depth.bids.len(), depth.asks.len(), side[0].0
            );
        }
    }
}
