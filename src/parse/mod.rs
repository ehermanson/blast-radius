use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
mod facts;
pub use facts::*;

mod javascript;
use javascript::parse_javascript_module;

#[cfg(any(feature = "vue", feature = "svelte"))]
mod component;
#[cfg(any(feature = "vue", feature = "svelte"))]
use component::parse_component_module;

#[cfg(feature = "java")]
mod java;
#[cfg(feature = "java")]
use java::parse_java_module;

#[cfg(feature = "ruby")]
mod ruby;
#[cfg(feature = "ruby")]
use ruby::parse_ruby_module;

#[cfg(feature = "python")]
mod python;
#[cfg(feature = "python")]
use python::parse_python_module;

#[cfg(feature = "rust")]
mod rust_lang;
#[cfg(feature = "rust")]
use rust_lang::parse_rust_module;

pub fn parse_module(path: &Path) -> Result<ModuleFacts> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read source file {}", path.display()))?;

    #[cfg(feature = "python")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("py") {
        return parse_python_module(path, &source);
    }

    #[cfg(feature = "rust")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
        return parse_rust_module(path, &source);
    }

    #[cfg(feature = "vue")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("vue") {
        return parse_component_module(path, &source, "vue");
    }

    #[cfg(feature = "svelte")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("svelte") {
        return parse_component_module(path, &source, "svelte");
    }

    #[cfg(feature = "ruby")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("rb") {
        return parse_ruby_module(path, &source);
    }

    #[cfg(feature = "java")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("java") {
        return parse_java_module(path, &source);
    }

    parse_javascript_module(path, &source)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::parse_module;

    #[test]
    fn parses_js_files_with_jsx() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("renderAvatar.js");
        fs::write(
            &path,
            r#"
import Avatar from '@mui/material/Avatar';

export function renderAvatar(params) {
  if (params.value == null) {
    return '';
  }

  return <Avatar>{params.value.name}</Avatar>;
}
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert_eq!(facts.imports.len(), 1);
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "renderAvatar")
        );
    }

    #[test]
    fn parses_modern_module_extensions() {
        let dir = tempdir().unwrap();

        let mjs_path = dir.path().join("widget.mjs");
        fs::write(&mjs_path, "export const widget = <div />;").unwrap();
        parse_module(&mjs_path).unwrap();

        let cts_path = dir.path().join("server.cts");
        fs::write(&cts_path, "export const server = 1;").unwrap();
        parse_module(&cts_path).unwrap();
    }

    #[cfg(feature = "python")]
    #[test]
    fn parses_python_imports_and_exports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("email.py");
        fs::write(
            &path,
            r#"
from ..models import User
from . import formatting

DEFAULT_TEMPLATE = "welcome"

def send_email(user: User) -> str:
    return formatting.format_subject(user.email, DEFAULT_TEMPLATE)
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "send_email")
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "..models" && import.local == "User")
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == ".formatting" && import.local == "formatting")
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn parses_rust_imports_exports_and_reexports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lib.rs");
        fs::write(
            &path,
            r#"
pub mod services;

use crate::models::User;
pub use crate::services::email::send_email;

pub struct App;
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(facts.exports.iter().any(|export| export.exported == "App"));
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "mod:services")
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "crate::models" && import.local == "User")
        );
        assert!(
            facts
                .reexports
                .iter()
                .any(|reexport| reexport.source == "crate::services::email"
                    && reexport.exported == "send_email")
        );
    }

    #[cfg(feature = "vue")]
    #[test]
    fn parses_vue_script_imports_and_default_export() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("Button.vue");
        fs::write(
            &path,
            r#"
<script setup lang="ts">
import { formatLabel } from './shared'
const label = formatLabel('save')
</script>
<template><button>{{ label }}</button></template>
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "./shared" && import.local == "formatLabel")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "default")
        );
    }

    #[cfg(feature = "svelte")]
    #[test]
    fn parses_svelte_script_imports_and_default_export() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("Card.svelte");
        fs::write(
            &path,
            r#"
<script lang="ts">
  import Button from './Button.vue'
  export let title = 'Settings'
</script>
<Button />
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "./Button.vue" && import.local == "Button")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "default")
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn parses_ruby_requires_and_exports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("email_service.rb");
        fs::write(
            &path,
            r#"
require_relative "../models/user"

class EmailService
  def self.send_email(email)
  end
end
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "../models/user")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "EmailService")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "send_email")
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn parses_java_imports_and_exports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("EmailService.java");
        fs::write(
            &path,
            r#"
package com.example.service;

import com.example.model.User;

public class EmailService {}
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "com.example.model.User" && import.local == "User")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "EmailService")
        );
    }
}
