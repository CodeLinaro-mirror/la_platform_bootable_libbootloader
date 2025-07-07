// Copyright 2025, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Utilities for controlling the UEFI TPL execution level.
//!
//! https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#event-timer-and-task-priority-services

use crate::defs::{EfiBootService, EfiTpl};

/// A trait to wrap raising and restoring the UEFI TPL.
///
/// See [TplScope] for a helper struct to use this trait.
#[cfg_attr(feature = "mocks", mockall::automock)]
pub trait TplControl {
    /// Raises the TPL.
    ///
    /// This should be used only to guard critical sections, most execution
    /// should be done at the lowest possible TPL.
    ///
    /// When the critical section is finished, call [restore_tpl()] with the
    /// value returned here.
    ///
    /// # Arguments
    ///
    /// * `tpl`: desired TPL
    ///
    /// # Returns
    ///
    /// The previous TPL. This function cannot fail.
    ///
    /// # Safety
    ///
    /// `tpl` must be a valid TPL level >= the current TPL, the UEFI spec states
    /// it is undefined behavior to attempt to lower the TPL with this function.
    ///
    /// Note that there is not an API to check the current TPL, but each
    /// protocol has a maximum allowed TPL, so within a protocol implementation
    /// it's safe to raise the TPL to the protocol's max or above, which amounts
    /// to entering the critical section for that protocol.
    unsafe fn raise_tpl(&self, tpl: EfiTpl) -> EfiTpl;

    /// Restores the TPL to a previous value.
    ///
    /// # Arguments
    ///
    /// * `tpl`: the TPL to restore
    ///
    /// # Safety
    ///
    /// The provided `tpl` must come from the previous paired call to
    /// [raise_tpl()].
    ///
    /// These calls may be nested, but raise/restore must be paired in LIFO
    /// ordering, e.g.:
    ///
    /// ```
    /// # use efi_types::{
    /// #     defs::{EFI_TPL_CALLBACK, EFI_TPL_NOTIFY},
    /// #     tpl::TplControl
    /// # };
    ///
    /// fn raise_twice<T: TplControl>(control: &T) {
    ///     // SAFETY: assume we start at the lowest `EFI_TPL_APPLICATION`.
    ///     let tpl_1 = unsafe { control.raise_tpl(EFI_TPL_CALLBACK) };
    ///     let tpl_2 = unsafe { control.raise_tpl(EFI_TPL_NOTIFY) };
    ///
    ///     // SAFETY: we're unstacking the TPL in LIFO order. It would be UB to
    ///     // switch the ordering or to skip `tpl_2`.
    ///     unsafe { control.restore_tpl(tpl_2) };
    ///     unsafe { control.restore_tpl(tpl_1) };
    /// }
    /// ```
    unsafe fn restore_tpl(&self, tpl: EfiTpl);
}

/// A [TplControl] implementation for the C [EfiBootService] struct.
impl TplControl for EfiBootService {
    unsafe fn raise_tpl(&self, tpl: EfiTpl) -> EfiTpl {
        // SAFETY: we've encoded the UEFI requirements in [raise_tpl()] docs.
        unsafe { self.raise_tpl.unwrap()(tpl) }
    }

    unsafe fn restore_tpl(&self, tpl: EfiTpl) {
        // SAFETY: we've encoded the UEFI requirements in [restore_tpl()] docs.
        unsafe { self.restore_tpl.unwrap()(tpl) }
    }
}

/// If a type T implements [TplControl], `&T` should also.
///
/// This allows [TplScope] to either borrow or own a [TplControl].
impl<T: TplControl> TplControl for &T {
    unsafe fn raise_tpl(&self, tpl: EfiTpl) -> EfiTpl {
        // SAFETY: forwarding to underlying object with the same safety properties.
        unsafe { (*self).raise_tpl(tpl) }
    }

    unsafe fn restore_tpl(&self, tpl: EfiTpl) {
        // SAFETY: forwarding to underlying object with the same safety properties.
        unsafe { (*self).restore_tpl(tpl) }
    }
}

/// A struct that raises the TPL on creation, and restores it on drop.
///
/// This can either borrow or own a [TplControl]. Borrowing will generally be
/// more useful for clients who will want to pass in a reference to the
/// [EfiBootService] struct, but providers may find it simpler to create and
/// pass ownership of a small or zero-sized object which directly modifies
/// system configuration to avoid dealing with borrow lifetimes.
pub struct TplScope<T: TplControl> {
    tpl_control: T,
    restore_tpl: EfiTpl,
}

/// Allows [TplScope] objects to stack on top of each other.
///
/// This holds a mutable borrow of the underlying [TplScope], which ensures that
/// they will be dropped in LIFO order and that each [TplScope] can only have a
/// single child.
pub struct ScopeStack<'a, T: TplControl> {
    previous_scope: &'a mut TplScope<T>,
}

/// Forwards [TplControl] calls into the underlying borrowed [TplScope].
impl<'a, T: TplControl> TplControl for ScopeStack<'a, T> {
    unsafe fn raise_tpl(&self, tpl: EfiTpl) -> EfiTpl {
        // SAFETY: forwarding to underlying object with the same safety properties.
        unsafe { self.previous_scope.tpl_control.raise_tpl(tpl) }
    }

    unsafe fn restore_tpl(&self, tpl: EfiTpl) {
        // SAFETY: forwarding to underlying object with the same safety properties.
        unsafe { self.previous_scope.tpl_control.restore_tpl(tpl) }
    }
}

impl<T: TplControl> TplScope<T> {
    /// Creates a new [TplScope].
    ///
    /// # Arguments
    ///
    /// * `tpl_control`: the TPL controller
    /// * `tpl`: desired TPL
    ///
    /// # Safety
    ///
    /// `tpl` must be a valid TPL level >= the current TPL, the UEFI spec states
    /// it is undefined behavior to attempt to lower the TPL with this function.
    ///
    /// Additionally, no other [TplScope] may exist in the caller's execution.
    /// See [TplScope::new_stacked()] for a version of this function that can be
    /// used with multiple [TplScopes].
    /// ```
    pub unsafe fn new(tpl_control: T, tpl: EfiTpl) -> Self {
        // SAFETY: function safety requires `tpl` >= the current TPL.
        let restore_tpl = unsafe { tpl_control.raise_tpl(tpl) };
        Self { tpl_control, restore_tpl }
    }

    /// Creates a new [TplScope] on top of an existing one.
    ///
    /// This uses Rust's lifetime and ownership enforcement to ensure proper
    /// LIFO ordering of multiple [TplScope] objects.
    ///
    /// If you only need one [TplScope], use [TplScope::new()] instead.
    ///
    /// # Arguments
    ///
    /// * `tpl_scope`: the [TplScope] to stack on top of
    /// * `tpl`: desired TPL
    ///
    /// # Safety
    ///
    /// `tpl` must be a valid TPL level >= the current TPL, the UEFI spec states
    /// it is undefined behavior to attempt to lower the TPL with this function.
    pub unsafe fn new_stacked<'a>(
        tpl_scope: &'a mut Self,
        tpl: EfiTpl,
    ) -> TplScope<ScopeStack<'a, T>> {
        // SAFETY: function safety requires `tpl` >= the current TPL.
        unsafe { TplScope::<ScopeStack<'a, T>>::new(ScopeStack { previous_scope: tpl_scope }, tpl) }
    }
}

impl<T: TplControl> Drop for TplScope<T> {
    fn drop(&mut self) {
        // SAFETY:
        // * `self.restore_tpl` is the value returned from `raise_tpl()`
        // * we know we're restoring the TPL in LIFO ordering because either:
        //   a) created via `new()`, so this is the last `TplScope` to drop
        //   b) created via `new_stacked()`, so the next `TplScope` on the stack
        //      still exists because we're borrowing it
        unsafe { self.tpl_control.restore_tpl(self.restore_tpl) };
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::defs::{EFI_TPL_APPLICATION, EFI_TPL_CALLBACK, EFI_TPL_NOTIFY};
    use std::{cell::RefCell, rc::Rc};

    /// A test helper to keep a stack of TPL states.
    ///
    /// All TPL modifications are safe here; any invalid usage e.g. trying to
    /// `raise_tpl()` to a lower TPL will panic instead of triggering UB.
    struct TplStack {
        tpl_stack: Vec<EfiTpl>,
    }

    impl TplStack {
        /// Starts at [EFI_TPL_APPLICATION].
        fn new() -> Self {
            Self { tpl_stack: vec![EFI_TPL_APPLICATION] }
        }

        /// Moves to a higher TPL.
        ///
        /// Panicks if `tpl` is lower than the current.
        fn push_tpl(&mut self, tpl: EfiTpl) -> EfiTpl {
            let previous_tpl = *self.tpl_stack.last().unwrap();
            assert!(tpl >= previous_tpl);
            self.tpl_stack.push(tpl);
            previous_tpl
        }

        /// Restores the next lower TPL.
        ///
        /// Panicks if `tpl` is not equal to the resulting lower TPL.
        fn pop_tpl(&mut self, tpl: EfiTpl) {
            assert!(self.tpl_stack.pop().is_some());
            assert_eq!(tpl, *self.tpl_stack.last().unwrap());
        }

        /// Returns the current TPL.
        fn get_tpl(&self) -> EfiTpl {
            *self.tpl_stack.last().unwrap()
        }
    }

    // Make sure `TplStack` behaves like we expect.
    #[test]
    fn fake_tpl_stack() {
        let mut stack = TplStack::new();
        assert_eq!(stack.tpl_stack, [EFI_TPL_APPLICATION]);

        stack.push_tpl(EFI_TPL_CALLBACK);
        assert_eq!(stack.tpl_stack, [EFI_TPL_APPLICATION, EFI_TPL_CALLBACK]);

        stack.pop_tpl(EFI_TPL_APPLICATION);
        assert_eq!(stack.tpl_stack, [EFI_TPL_APPLICATION]);
    }

    #[test]
    #[should_panic]
    fn fake_tpl_stack_panics_on_misuse() {
        let mut stack = TplStack::new();
        stack.push_tpl(EFI_TPL_CALLBACK);
        stack.push_tpl(EFI_TPL_APPLICATION);
    }

    #[test]
    fn tpl_control_for_efi_boot_service() {
        // Create some data that can be modified from raw C functions.
        thread_local! {
            static TPL_STACK: RefCell<TplStack> = RefCell::new(TplStack::new());
        }
        unsafe extern "efiapi" fn raise_tpl(new_tpl: EfiTpl) -> EfiTpl {
            TPL_STACK.with_borrow_mut(|s| s.push_tpl(new_tpl))
        }
        unsafe extern "efiapi" fn restore_tpl(old_tpl: EfiTpl) {
            TPL_STACK.with_borrow_mut(|s| s.pop_tpl(old_tpl))
        }

        // Hook the C functions into an `EfiBootService` so we can verify our
        // `TplControl` trait implementation is working properly.
        let boot_service = EfiBootService {
            raise_tpl: Some(raise_tpl),
            restore_tpl: Some(restore_tpl),
            ..Default::default()
        };

        assert_eq!(TPL_STACK.with_borrow(|s| s.get_tpl()), EFI_TPL_APPLICATION);

        // SAFETY: `TplStack` is safe for any TPL value.
        let old_tpl = unsafe { TplControl::raise_tpl(&boot_service, EFI_TPL_CALLBACK) };
        assert_eq!(old_tpl, EFI_TPL_APPLICATION);
        assert_eq!(TPL_STACK.with_borrow(|s| s.get_tpl()), EFI_TPL_CALLBACK);

        // SAFETY: `TplStack` is safe for any TPL value.
        unsafe { TplControl::restore_tpl(&boot_service, old_tpl) };
        assert_eq!(TPL_STACK.with_borrow(|s| s.get_tpl()), EFI_TPL_APPLICATION);
    }

    /// Returns a [TplStack] and a [MockTplControl] that will call into it.
    fn create_tpl_stack_and_mock() -> (Rc<RefCell<TplStack>>, MockTplControl) {
        let stack = Rc::new(RefCell::new(TplStack::new()));
        let mut mock = MockTplControl::new();
        let raise_stack = stack.clone();
        mock.expect_raise_tpl().returning_st(move |tpl| raise_stack.borrow_mut().push_tpl(tpl));
        let restore_stack = stack.clone();
        mock.expect_restore_tpl().returning_st(move |tpl| restore_stack.borrow_mut().pop_tpl(tpl));
        (stack, mock)
    }

    #[test]
    fn tpl_scope_borrows_control() {
        let (stack, mock) = create_tpl_stack_and_mock();
        {
            // SAFETY:
            // * `mock` is backed by `TplStack` which always safe
            // * there is no other `TplScope` active
            let _scope = unsafe { TplScope::new(&mock, EFI_TPL_NOTIFY) };
            assert_eq!(stack.borrow().get_tpl(), EFI_TPL_NOTIFY);
        }
        assert_eq!(stack.borrow().get_tpl(), EFI_TPL_APPLICATION);
    }

    #[test]
    fn tpl_scope_owns_control() {
        let (stack, mock) = create_tpl_stack_and_mock();
        {
            // SAFETY:
            // * `mock` is backed by `TplStack` which always safe
            // * there is no other `TplScope` active
            let _scope = unsafe { TplScope::new(mock, EFI_TPL_NOTIFY) };
            assert_eq!(stack.borrow().get_tpl(), EFI_TPL_NOTIFY);
        }
        assert_eq!(stack.borrow().get_tpl(), EFI_TPL_APPLICATION);
    }

    #[test]
    fn tpl_scope_stack_borrows_control() {
        let (stack, mock) = create_tpl_stack_and_mock();
        {
            // SAFETY:
            // * `mock` is backed by `TplStack` which always safe
            // * there is no other `TplScope` active
            let mut scope = unsafe { TplScope::new(&mock, EFI_TPL_CALLBACK) };
            assert_eq!(stack.borrow().get_tpl(), EFI_TPL_CALLBACK);
            {
                // SAFETY: `scope` is backed by `TplStack` which always safe.
                let _scope = unsafe { TplScope::new_stacked(&mut scope, EFI_TPL_NOTIFY) };
                assert_eq!(stack.borrow().get_tpl(), EFI_TPL_NOTIFY);
            }
            assert_eq!(stack.borrow().get_tpl(), EFI_TPL_CALLBACK);
        }
        assert_eq!(stack.borrow().get_tpl(), EFI_TPL_APPLICATION);
    }

    #[test]
    fn tpl_scope_stack_owns_control() {
        let (stack, mock) = create_tpl_stack_and_mock();
        {
            // SAFETY:
            // * `mock` is backed by `TplStack` which always safe
            // * there is no other `TplScope` active
            let mut scope = unsafe { TplScope::new(mock, EFI_TPL_CALLBACK) };
            assert_eq!(stack.borrow().get_tpl(), EFI_TPL_CALLBACK);
            {
                // The stacked scope never owns anything, it always borrows
                // the underlying [TplScope] and calls into its [TplControl].
                // SAFETY: `scope` is backed by `TplStack` which always safe.
                let _scope = unsafe { TplScope::new_stacked(&mut scope, EFI_TPL_NOTIFY) };
                assert_eq!(stack.borrow().get_tpl(), EFI_TPL_NOTIFY);
            }
            assert_eq!(stack.borrow().get_tpl(), EFI_TPL_CALLBACK);
        }
        assert_eq!(stack.borrow().get_tpl(), EFI_TPL_APPLICATION);
    }
}
