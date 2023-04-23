use hearth_guest::{wasm::WasmSpawnInfo, LocalProcessId, ProcessId};

fn get_spawner() -> ProcessId {
    let this_peer = hearth_guest::this_pid().split().0;
    hearth_guest::service_lookup(this_peer, "hearth.cognito.WasmProcessSpawner")
        .expect("Couldn't find Wasm spawner service")
}

fn send<T: serde::Serialize>(pid: ProcessId, data: &T) {
    let msg = serde_json::to_vec(data).unwrap();
    hearth_guest::send(pid, &msg);
}

fn recv<T: for<'a> serde::Deserialize<'a>>() -> (ProcessId, T) {
    let msg = hearth_guest::recv();
    let sender = msg.get_sender();
    let data = serde_json::from_slice(&msg.get_data()).unwrap();
    (sender, data)
}

fn spawn(spawner: ProcessId, cb: fn()) -> ProcessId {
    send(
        spawner,
        &WasmSpawnInfo {
            lump: hearth_guest::this_lump(),
            entrypoint: Some(cb as usize as u32),
        },
    );

    let msg = String::from_utf8(hearth_guest::recv().get_data()).unwrap();
    let local_pid: u32 = msg.parse().unwrap();
    ProcessId::from_peer_process(spawner.split().0, LocalProcessId(local_pid))
}

#[no_mangle]
pub extern "C" fn run() {
    let spawner = get_spawner();
    let input: Vec<u64> = (0..100000).into_iter().rev().collect();
    merge_sort(spawner, input);
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Data {
    pub inner: Vec<u64>,
}

fn bubble_sort(input: &mut Vec<u64>) {
    let mut swapped = true;
    while swapped {
        swapped = false;
        for idx in 0..(input.len() - 1) {
            if input[idx] >= input[idx + 1] {
                input.swap(idx, idx + 1);
                swapped = true;
            }
        }
    }
}

fn merge_sort_outer() {
    let spawner = get_spawner();
    let (parent, input) = recv::<Data>();
    let result = merge_sort(spawner, input.inner);
    send(parent, &Data { inner: result });
}

fn merge_sort(spawner: ProcessId, mut input: Vec<u64>) -> Vec<u64> {
    if input.len() > 32 {
        let a_pid = spawn(spawner, merge_sort_outer);
        let b_pid = spawn(spawner, merge_sort_outer);

        let (a, b) = input.split_at(input.len() / 2);
        send(a_pid, &Data { inner: a.to_vec() });
        send(b_pid, &Data { inner: b.to_vec() });

        let (_sender, a_sorted) = recv::<Data>();
        let (_sender, b_sorted) = recv::<Data>();
        merge(a_sorted.inner, b_sorted.inner)
    } else {
        bubble_sort(&mut input);
        input
    }
}

fn merge(mut a: Vec<u64>, b: Vec<u64>) -> Vec<u64> {
    // blech i can do this later
    a.extend_from_slice(&b);
    a.sort_unstable();
    a
}
