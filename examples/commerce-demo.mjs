// Synthetic diagram source. No network requests, payments, or messages occur.
export async function submit_order(order) {
  await validate_order(order);
  await process_payment(order);
  await create_shipment(order);
}

export async function validate_order(order) {
  return Promise.all([check_stock(order), price_order(order)]);
}

export function check_stock(order) { return { available: true, order }; }
export function price_order(order) { return { total: 120, order }; }

export async function process_payment(order) {
  const risk = risk_check(order);
  if (risk.accepted) return charge_card(order);
}

export function risk_check(order) { return { accepted: true, order }; }

export async function charge_card(order) {
  store_receipt(order);
  return publish_event({ kind: "charged", order });
}

export function store_receipt(value) { return { saved: true, value }; }

export async function publish_event(event) {
  return notify_customer(event);
}

export async function notify_customer(event) {
  return Promise.all([send_email(event), send_sms(event)]);
}

export function send_email(event) { return { simulated: true, event }; }
export function send_sms(event) { return { simulated: true, event }; }
export function legacy_notify(event) { return { simulated: true, event }; }

export async function create_shipment(order) {
  reserve_stock(order);
  return book_carrier(order);
}

export function reserve_stock(order) { return { reserved: true, order }; }
export function book_carrier(order) { return { tracking: "SAMPLE-ONLY", order }; }

export async function retry_payments() {
  for (const order of load_retry_queue()) {
    for (let attempt = 0; attempt < 3; attempt++) {
      await process_payment(order);
    }
  }
}

export function load_retry_queue() { return [{ id: "sample", total: 120 }]; }

export async function refund_order(id) {
  const payment = load_payment(id);
  if (payment.settled) return refund_payment(payment);
}

export function load_payment(id) { return { id, settled: true }; }

export async function refund_payment(payment) {
  store_receipt(payment);
  return publish_event({ kind: "refunded", payment });
}
