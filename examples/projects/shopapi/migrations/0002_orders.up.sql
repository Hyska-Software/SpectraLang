CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    customer TEXT NOT NULL,
    status INTEGER NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    total_cents INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE order_items (
    order_id INTEGER NOT NULL,
    product_id INTEGER NOT NULL,
    sku TEXT NOT NULL,
    title TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    unit_price_cents INTEGER NOT NULL,
    PRIMARY KEY (order_id, product_id)
);

CREATE INDEX order_items_order_id ON order_items(order_id);
