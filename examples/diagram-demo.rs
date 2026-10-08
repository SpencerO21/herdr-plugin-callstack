// Sample source for the diagram demonstration. It does not make real charges.
fn submit_order(total: u32) {
    if total > 0 {
        charge_card(total);
    }
    send_receipt();
}

fn retry_payment(total: u32) {
    for _attempt in 0..3 {
        charge_card(total);
    }
}

fn charge_card(_amount: u32) {
    save_result();
}

fn send_receipt() {}

fn save_result() {}

fn main() {
    submit_order(10);
    retry_payment(10);
}
