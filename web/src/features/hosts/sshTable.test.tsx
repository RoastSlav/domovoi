import { render, screen, within } from "@testing-library/react";
import { T0 } from "../../test/fixtures";
import { displayAddress } from "./ipAddress";
import { SshLoginTable } from "./SshLoginTable";
import type { LoginEntry } from "./sshLogins";

const entry = (id: string, address: string, network: string): LoginEntry => ({
  id,
  user: id,
  address,
  network,
  at: T0,
  count: 1,
  result: "accepted",
});

test("ipv6_addresses_show_in_the_canonical_compressed_form", () => {
  expect(displayAddress("2001:db8:85a3:0:0:8a2e:370:7334")).toBe("2001:db8:85a3::8a2e:370:7334");
  expect(displayAddress("2001:0DB8:0000:0000:0000:0000:0000:0001")).toBe("2001:db8::1");
  expect(displayAddress("0:0:0:0:0:0:0:1")).toBe("::1");
  expect(displayAddress("2001:db8:0:1:0:0:0:1")).toBe("2001:db8:0:1::1");
  expect(displayAddress("2001:db8:0:1:1:1:1:1")).toBe("2001:db8:0:1:1:1:1:1");
  expect(displayAddress("fe80::1")).toBe("fe80::1");
  expect(displayAddress("192.168.1.20")).toBe("192.168.1.20");
  expect(displayAddress("not an address")).toBe("not an address");
});

test("the_network_has_its_own_column_and_the_address_stands_alone_in_mono", () => {
  const rows = [
    entry("alice", "192.168.1.20", "lan"),
    entry("bob", "2001:db8:85a3:0:0:8a2e:370:7334", "wan"),
  ];
  render(<SshLoginTable all={rows} shown={rows} host="nas" />);

  const table = screen.getByRole("table", { name: "SSH logins on nas" });
  expect(table).toHaveAttribute("data-layout", "fixed");
  expect(within(table).getByRole("columnheader", { name: "Network" })).toBeVisible();
  const bob = within(table).getByText("bob").closest("tr") as HTMLElement;
  const cells = within(bob).getAllByRole("cell");
  expect(cells[1]).toHaveTextContent(/^2001:db8:85a3::8a2e:370:7334$/);
  expect(cells[2]).toHaveTextContent(/^Internet$/);
  expect(cells[1]).toHaveAttribute("data-truncate", "ellipsis");
  const alice = within(table).getByText("alice").closest("tr") as HTMLElement;
  expect(within(alice).getAllByRole("cell")[2]).toHaveTextContent(/^Local network$/);
});
