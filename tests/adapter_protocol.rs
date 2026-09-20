use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn expert_mode_is_explicit_and_refreshes_without_restarting_the_adapter() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_triangle-adapter"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let modes = [Value::Null, json!(true), json!(false), json!("true")];
    {
        let mut input = child.stdin.take().unwrap();
        writeln!(
            input,
            "{}",
            json!({"type":"config","boardWidth":4,"boardHeight":26,"kicks":"SRS-X"})
        )
        .unwrap();
        for mode in &modes {
            let data = if mode.is_null() {
                json!({})
            } else {
                json!({"expertMode":mode})
            };
            writeln!(input,"{}",json!({"type":"state","board":[["G","G","G",null],["G","G","G",null]],"current":"T","hold":"I","queue":["O","S","Z","L","J"],"combo":4,"b2b":-1,"garbage":[],"data":data})).unwrap();
            writeln!(input, "{}", json!({"type":"play"})).unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let moves: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|v| v["type"] == "move")
        .collect();
    assert_eq!(moves.len(), 4);
    for (m, expected) in moves.iter().zip([false, true, false, false]) {
        assert_eq!(m["data"]["expertMode"], expected);
        assert_eq!(m["keys"].as_array().unwrap().last().unwrap(), "hardDrop");
    }
    assert_eq!(moves[0]["keys"], moves[2]["keys"]);
}
#[test]
fn online_play_uses_queue_defense_and_refreshes_back_to_pc() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_triangle-adapter"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        writeln!(
            input,
            "{}",
            json!({"type":"config","boardWidth":4,"boardHeight":26,"kicks":"SRS-X","spins":"all"})
        )
        .unwrap();
        for incoming in [8, 0] {
            writeln!(input, "{}", json!({"type":"state",
                "board":[["G","G",null,null],["G","G",null,null]],
                "current":"O","hold":"T","queue":["S"],"combo":7,"b2b":-1,
                "garbage":[incoming],"data":{"garbageContext":{
                    "packets":if incoming>0 {vec![json!({"amount":incoming,"readyIn":0})]} else {vec![]},
                    "nextLockFrames":8,"framesPerPiece":38,"cap":8
                }}
            })).unwrap();
            writeln!(input, "{}", json!({"type":"play","garbageCap":8})).unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let moves: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|m| m["type"] == "move")
        .collect();
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0]["data"]["incoming"], 8);
    assert!(moves[0]["data"]["strategy"]
        .as_str()
        .unwrap()
        .starts_with("PC defense: cancel 8 next"));
    assert_eq!(moves[1]["data"]["incoming"], 0);
    assert_eq!(moves[1]["data"]["strategy"], "PC in 1 placements");
    for m in moves {
        assert_eq!(m["keys"].as_array().unwrap().last().unwrap(), "hardDrop");
    }
}

#[test]
fn adapter_refuses_non_srs_x_configurations() {
    for kicks in ["SRS", "SRS+", "unknown"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_triangle-adapter"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "{}",
            json!({"type":"config","boardWidth":4,"kicks":kicks})
        )
        .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8(result.stdout)
            .unwrap()
            .contains("hardDrop"));
    }
}

#[test]
fn room_pc_and_combo_values_are_used_without_rejecting_handheld() {
    for bonus in [0, 5, 10] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_triangle-adapter"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        {
            let mut input = child.stdin.take().unwrap();
            writeln!(input,"{}",json!({"type":"config","boardWidth":4,"boardHeight":26,"kicks":"SRS-X","spins":"handheld","comboTable":"none","pcGarbage":bonus})).unwrap();
            writeln!(input,"{}",json!({"type":"state","board":[["G","G",null,null],["G","G",null,null]],"current":"O","hold":null,"queue":[],"combo":10,"b2b":-1,"garbage":[]})).unwrap();
            writeln!(input, "{}", json!({"type":"play"})).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        let m = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .find(|m| m["type"] == "move")
            .unwrap();
        assert_eq!(m["data"]["expectedAttack"], 1 + bonus);
    }
}
