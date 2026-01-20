# Contributing to TurboPlot

Thank you for your interest in contributing to TurboPlot! This guide provides basic instructions on setting up your development environment and creating a release.

## Release Process

To create a new release, follow these steps:

1.  **Update the Version**:
    Bump the version number in `Cargo.toml`:
    ```toml
    [package]
    version = "1.2.3" # Update this
    ```

2.  **Commit the Change**:
    ```bash
    git add Cargo.toml
    git commit -m "chore: bump version to 1.2.3"
    git push origin master
    ```

3.  **Publish to crates.io**:
    Ensure you are logged in (`cargo login`) and have the necessary permissions.
    ```bash
    cargo publish
    ```

4.  **Create and Push a Tag**:
    The release workflow is triggered by tags starting with `v`.
    ```bash
    git tag v1.2.3
    git push origin v1.2.3
    ```

5.  **Automatic Build**:
    The GitHub Actions workflow will automatically:
    - Build `turboplot` for Linux, Windows, and macOS.
    - Create a GitHub Release for `v1.2.3`.
    - Upload the built binaries as release assets.
