// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::s2sdk;
use plugify::Str;

#[derive(Clone, Default)]
pub struct Probe {
    missing: Vec<String>,
}

impl Probe {
    pub fn require(&mut self, available: bool, requirement: impl Into<String>) {
        if !available {
            let requirement = requirement.into();
            if !self.missing.contains(&requirement) {
                self.missing.push(requirement);
            }
        }
    }

    pub fn schema(&mut self, class: &str, field: &str) {
        let lookup = unsafe { s2sdk::__s2sdk_GetSchemaOffset };
        self.require(lookup.is_some(), "import:GetSchemaOffset");
        if let Some(lookup) = lookup {
            self.require(
                unsafe { lookup(&Str::from_str(class), &Str::from_str(field)) } >= 0,
                format!("schema:{class}.{field}"),
            );
        }
    }

    pub fn include(&mut self, other: Self) {
        for missing in other.missing {
            self.require(false, missing);
        }
    }

    pub fn available(&self) -> bool {
        self.missing.is_empty()
    }

    pub fn record(self, name: &'static str, reasons: &mut Vec<String>) -> bool {
        if self.available() {
            true
        } else {
            reasons.push(format!("{name}:[{}]", self.missing.join(",")));
            false
        }
    }
}

macro_rules! imports {
    ($probe:expr; $($name:ident),+ $(,)?) => {{
        $( $probe.require(
            unsafe { std::ptr::addr_of!(crate::s2sdk::$name).read().is_some() },
            concat!("import:", stringify!($name)).replace("__s2sdk_", ""),
        ); )+
    }};
}
pub(crate) use imports;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_requirements_keep_their_reason_without_duplicates() {
        let mut common = Probe::default();
        common.require(false, "schema:shared.field");
        let mut dependent = common.clone();
        dependent.include(common);
        dependent.require(true, "import:present");
        dependent.require(false, "import:missing");
        let mut reasons = Vec::new();
        assert!(!dependent.record("dependent", &mut reasons));
        assert_eq!(reasons, ["dependent:[schema:shared.field,import:missing]"]);
        assert!(Probe::default().record("available", &mut reasons));
        assert_eq!(reasons.len(), 1);
    }
}
