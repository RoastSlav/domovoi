# Quick start

This starts the development hub with a temporary database, two fake hosts, a seeded
incident and proposed actions. It is a UI preview; it does not enroll a real agent or
connect Discord or an LLM provider.

You need the Rust toolchain from `rust-toolchain.toml` and Node.js 22.

1. From the repository root, start the example hub:

   ```sh
   cargo run -p domovoi-hub --example dev_hub
   ```

   It listens on `127.0.0.1:7420` and prints a one-time setup link.
2. In another terminal, start the web development server:

   ```sh
   cd web
   npm ci
   npm run dev
   ```

3. Open the setup link after changing its port from `7420` to `5173`. Create the local
   admin account when prompted.
4. Press Enter in the hub terminal to send another fake incident and watch it appear.

The example uses temporary data and is not suitable for monitoring real hosts. The
production hub and agent commands, release binaries and deployment files are still
planned. The source checkout's `web/README.md` contains more UI development options.
