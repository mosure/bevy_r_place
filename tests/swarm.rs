use std::time::Duration;

use bevy_r_place::prelude::*;
use tokio::{
    select,
    time::sleep,
};


pub async fn wait_for_subscription(
    handle: &BevyPlaceNodeHandle,
    timeout_duration: Duration,
) -> Result<(), String> {
    select! {
        received = handle.sub_rx.recv() => {
            match received {
                Ok(_peer_id) => Ok(()),
                Err(e) => Err(format!("Error receiving from channel: {e}")),
            }
        }
        _ = sleep(timeout_duration) => {
            Err("Timed out waiting for subscription.".to_string())
        }
    }
}


#[tokio::test]
async fn test_multi_node() {
    let (node1, node1_handle) = build_node(BevyPlaceNodeConfig::default()).expect("failed to build node");
    let (node2, node2_handle) = build_node(BevyPlaceNodeConfig::default()).expect("failed to build node");

    let handle1 = tokio::spawn(run_swarm_task(node1));
    let handle2 = tokio::spawn(run_swarm_task(node2));

    let timeout = Duration::from_secs(5);
    wait_for_subscription(&node1_handle, timeout).await.unwrap();
    wait_for_subscription(&node2_handle, timeout).await.unwrap();

    sleep(timeout).await;

    let msg_from_1 = PixelUpdateMsg {
        x: 42,
        y: 24,
        r: 123,
        g: 45,
        b: 67,
        timestamp: 1111,
        owner: [1; 32],
    };
    node1_handle.outbound_tx.send(msg_from_1.clone()).await.expect("Failed to send message");

    let msg_from_2 = PixelUpdateMsg {
        x: 10,
        y: 20,
        r: 111,
        g: 222,
        b: 33,
        timestamp: 2222,
        owner: [2; 32],
    };
    node2_handle.outbound_tx.send(msg_from_2.clone()).await.expect("Failed to send message");

    sleep(timeout).await;

    let mut found_msg_from_1 = false;
    while let Ok(msg) = node2_handle.inbound_rx.try_recv() {
        if msg == msg_from_1 {
            found_msg_from_1 = true;
            break;
        }
    }
    assert!(found_msg_from_1, "swarm2 did not receive the message from swarm1");

    let mut found_msg_from_2 = false;
    while let Ok(msg) = node1_handle.inbound_rx.try_recv() {
        if msg == msg_from_2 {
            found_msg_from_2 = true;
            break;
        }
    }
    assert!(found_msg_from_2, "swarm1 did not receive the message from swarm2");

    handle1.abort();
    handle2.abort();
}
