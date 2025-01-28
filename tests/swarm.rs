use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use bevy_r_place::prelude::*;
use serial_test::serial;
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
                Err(e) => Err(format!("error receiving from channel: {e}")),
            }
        }
        _ = sleep(timeout_duration) => {
            Err("timed out waiting for subscription.".to_string())
        }
    }
}

pub async fn wait_for_listening_addr(
    handle: &BevyPlaceNodeHandle,
    timeout_duration: Duration,
) -> Result<(), String> {
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);

    loop {
        {
            let addrs = handle.listening_addrs.lock().unwrap();
            if !addrs.is_empty() {
                sleep(Duration::from_secs(2)).await;
                return Ok(());
            }
        }

        if start.elapsed() >= timeout_duration {
            return Err("timed out waiting for listening address.".to_string());
        }

        sleep(poll_interval).await;
    }
}


// TODO: instead of serializing tests, isolate each network (e.g. bootstrap id /w mdns network IDs)
#[serial]
#[tokio::test]
async fn test_two_nodes() {
    let (node1, node1_handle) = build_node(BevyPlaceNodeConfig::default())
        .await
        .expect("failed to build node");
    let (node2, node2_handle) = build_node(BevyPlaceNodeConfig::default())
        .await
        .expect("failed to build node");

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
    node1_handle.outbound_tx.send(msg_from_1.clone()).await.expect("failed to send message");

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


#[serial_test::serial]
#[tokio::test]
async fn test_n_nodes_with_bootstrap() {
    let num_nodes = 30;
    let timeout = Duration::from_secs(8);

    let bootstrap_config = BevyPlaceNodeConfig::default();
    let (bootstrap_node, bootstrap_handle) = build_node(bootstrap_config)
        .await
        .expect("failed to build bootstrap node");
    let bootstrap_join_handle = tokio::spawn(run_swarm_task(bootstrap_node));

    wait_for_listening_addr(&bootstrap_handle, timeout).await
        .expect("bootstrap node did not confirm listening address in time");

    let bootstrap_addrs = {
        bootstrap_handle.listening_addrs.lock().unwrap().clone()
    };
    assert!(!bootstrap_addrs.is_empty(), "bootstrap node has no listening addresses");
    let bootstrap_addr = bootstrap_addrs[0].clone();

    let mut join_handles = Vec::with_capacity(num_nodes - 1);
    let mut node_handles = Vec::with_capacity(num_nodes - 1);

    for i in 0..(num_nodes - 1) {
        let config = BevyPlaceNodeConfig {
            bootstrap_peers: vec![bootstrap_addr.clone()],
            ..Default::default()
        };

        let (node, handle) = build_node(config).await.expect("Failed to build node");
        let join_handle = tokio::spawn(run_swarm_task(node));
        join_handles.push(join_handle);
        node_handles.push(handle);

        if i == (num_nodes - 1) / 2 {
            sleep(Duration::from_secs(10)).await;
        } else {
            sleep(Duration::from_millis(100)).await;
        }
    }

    for handle in &node_handles {
        wait_for_subscription(handle, timeout).await
            .expect("node did not confirm subscription in time");
    }

    sleep(Duration::from_secs(5)).await;

    let mut all_handles = vec![bootstrap_handle];
    all_handles.extend(node_handles);

    for (i, handle) in all_handles.iter().enumerate() {
        let msg = PixelUpdateMsg {
            x: i as u32,
            y: i as u32,
            r: (i * 10) as u8,
            g: (i * 20) as u8,
            b: (i * 30) as u8,
            timestamp: 1000 + i as u64,
            owner: [i as u8; 32],
        };
        handle.outbound_tx.send(msg).await
            .expect("failed to send outbound message");
    }

    sleep(Duration::from_secs(5)).await;

    let mut received_msgs_per_node = vec![Vec::new(); num_nodes];
    for (i, handle) in all_handles.iter().enumerate() {
        while let Ok(msg) = handle.inbound_rx.try_recv() {
            received_msgs_per_node[i].push(msg);
        }
    }

    let expected_owners: Vec<[u8; 32]> = (0..num_nodes).map(|n| [n as u8; 32]).collect();

    for (node_idx, received_msgs) in received_msgs_per_node.iter().enumerate() {
        let owners_received: HashSet<[u8; 32]> =
            received_msgs.iter().map(|msg| msg.owner).collect();

        for owner in &expected_owners {
            if owner == &[node_idx as u8; 32] {
                continue;
            }
            assert!(
                owners_received.contains(owner),
                "node {node_idx} did not receive message from owner: {owner:?}",
            );
        }
    }

    bootstrap_join_handle.abort();
    for handle in join_handles {
        handle.abort();
    }
}
