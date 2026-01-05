use color_eyre::eyre::Result;
use core_foundation_sys::base::{kCFAllocatorDefault, kCFAllocatorNull, CFRelease, CFTypeRef};
use core_foundation_sys::dictionary::{
    CFDictionaryCreateMutableCopy, CFDictionaryGetCount, CFDictionaryGetValue, CFDictionaryRef,
    CFMutableDictionaryRef,
};
use core_foundation_sys::string::{
    kCFStringEncodingUTF8, CFStringCreateWithBytesNoCopy, CFStringGetCString, CFStringRef,
};
use std::ffi::c_void;
use std::process::Command;
use std::ptr::null;
use std::time::{Duration, Instant};

type IOReportSubscriptionRef = *const c_void;
type CFArrayRef = *const c_void;

#[link(name = "IOReport", kind = "dylib")]
extern "C" {
    fn IOReportCopyChannelsInGroup(
        a: CFStringRef,
        b: CFStringRef,
        c: u64,
        d: u64,
        e: u64,
    ) -> CFDictionaryRef;

    fn IOReportCreateSubscription(
        a: *const c_void,
        b: CFMutableDictionaryRef,
        c: *mut CFMutableDictionaryRef,
        d: u64,
        e: *const c_void,
    ) -> IOReportSubscriptionRef;

    fn IOReportCreateSamples(
        a: IOReportSubscriptionRef,
        b: CFMutableDictionaryRef,
        c: *const c_void,
    ) -> CFDictionaryRef;

    fn IOReportCreateSamplesDelta(
        a: CFDictionaryRef,
        b: CFDictionaryRef,
        c: *const c_void,
    ) -> CFDictionaryRef;

    fn IOReportChannelGetChannelName(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetUnitLabel(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportSimpleGetIntegerValue(a: CFDictionaryRef, b: i32) -> i64;
}

extern "C" {
    fn CFArrayGetCount(arr: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: isize) -> *const c_void;
}

fn cfstr(val: &str) -> CFStringRef {
    unsafe {
        CFStringCreateWithBytesNoCopy(
            kCFAllocatorDefault,
            val.as_ptr(),
            val.len() as isize,
            kCFStringEncodingUTF8,
            0,
            kCFAllocatorNull,
        )
    }
}

fn from_cfstr(val: CFStringRef) -> String {
    if val.is_null() {
        return String::new();
    }
    unsafe {
        let mut buf = [0i8; 128];
        if CFStringGetCString(val, buf.as_mut_ptr(), 128, kCFStringEncodingUTF8) == 0 {
            return String::new();
        }
        std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .to_string()
    }
}

fn cfdict_get_val(dict: CFDictionaryRef, key: &str) -> Option<CFTypeRef> {
    unsafe {
        let key = cfstr(key);
        let val = CFDictionaryGetValue(dict, key as _);
        CFRelease(key as _);
        if val.is_null() {
            None
        } else {
            Some(val)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    LowPower,
    Automatic,
    HighPerformance,
    Unknown,
}

struct IOReportIterator {
    sample: CFDictionaryRef,
    items: CFArrayRef,
    index: isize,
    count: isize,
}

impl IOReportIterator {
    fn new(sample: CFDictionaryRef) -> Option<Self> {
        let items = cfdict_get_val(sample, "IOReportChannels")? as CFArrayRef;
        let count = unsafe { CFArrayGetCount(items) };
        Some(Self {
            sample,
            items,
            index: 0,
            count,
        })
    }
}

impl Drop for IOReportIterator {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.sample as _);
        }
    }
}

struct ChannelData {
    name: String,
    unit: String,
    value: i64,
}

impl Iterator for IOReportIterator {
    type Item = ChannelData;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }

        let item =
            unsafe { CFArrayGetValueAtIndex(self.items, self.index) } as CFDictionaryRef;
        self.index += 1;

        if item.is_null() {
            return self.next();
        }

        let name = from_cfstr(unsafe { IOReportChannelGetChannelName(item) });
        let unit = from_cfstr(unsafe { IOReportChannelGetUnitLabel(item) })
            .trim()
            .to_string();
        let value = unsafe { IOReportSimpleGetIntegerValue(item, 0) };

        Some(ChannelData { name, unit, value })
    }
}

struct IOReportSubscription {
    subscription: IOReportSubscriptionRef,
    channels: CFMutableDictionaryRef,
}

impl IOReportSubscription {
    fn new() -> Option<Self> {
        unsafe {
            let group = cfstr("Energy Model");
            let chan = IOReportCopyChannelsInGroup(group, null(), 0, 0, 0);
            CFRelease(group as _);

            if chan.is_null() {
                return None;
            }

            if cfdict_get_val(chan, "IOReportChannels").is_none() {
                CFRelease(chan as _);
                return None;
            }

            let count = CFDictionaryGetCount(chan);
            let channels = CFDictionaryCreateMutableCopy(kCFAllocatorDefault, count, chan);
            CFRelease(chan as _);

            if channels.is_null() {
                return None;
            }

            let mut sub_dict: CFMutableDictionaryRef = null::<c_void>() as _;
            let subscription =
                IOReportCreateSubscription(null(), channels, &mut sub_dict, 0, null());

            if subscription.is_null() {
                CFRelease(channels as _);
                return None;
            }

            Some(Self {
                subscription,
                channels,
            })
        }
    }

    fn sample(&self) -> Option<CFDictionaryRef> {
        let sample =
            unsafe { IOReportCreateSamples(self.subscription, self.channels, null()) };
        if sample.is_null() {
            None
        } else {
            Some(sample)
        }
    }
}

impl Drop for IOReportSubscription {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.channels as _);
        }
    }
}

pub struct PowerData {
    subscription: Option<IOReportSubscription>,
    last_sample: Option<CFDictionaryRef>,
    last_sample_time: Option<Instant>,
    cpu_power: f32,
    gpu_power: f32,
    ane_power: f32,
    total_system_power: f32,
    power_mode: PowerMode,
}

impl PowerData {
    pub fn new() -> Result<Self> {
        let subscription = IOReportSubscription::new();
        let last_sample = subscription.as_ref().and_then(|s| s.sample());
        let last_sample_time = last_sample.map(|_| Instant::now());

        let mut data = Self {
            subscription,
            last_sample,
            last_sample_time,
            cpu_power: 0.0,
            gpu_power: 0.0,
            ane_power: 0.0,
            total_system_power: 0.0,
            power_mode: PowerMode::Unknown,
        };

        data.refresh_power_mode();
        Ok(data)
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.refresh_power_metrics();
        self.refresh_power_mode();
        Ok(())
    }

    fn refresh_power_metrics(&mut self) {
        let Some(ref subscription) = self.subscription else {
            self.fallback_power_estimate();
            return;
        };

        let Some(current_sample) = subscription.sample() else {
            self.fallback_power_estimate();
            return;
        };

        let (Some(prev_sample), Some(prev_time)) = (self.last_sample, self.last_sample_time)
        else {
            self.last_sample = Some(current_sample);
            self.last_sample_time = Some(Instant::now());
            return;
        };

        let elapsed = prev_time.elapsed();
        self.calculate_power_from_delta(prev_sample, current_sample, elapsed);

        unsafe {
            CFRelease(prev_sample as _);
        }

        self.last_sample = Some(current_sample);
        self.last_sample_time = Some(Instant::now());
    }

    fn calculate_power_from_delta(
        &mut self,
        prev: CFDictionaryRef,
        current: CFDictionaryRef,
        elapsed: Duration,
    ) {
        let delta = unsafe { IOReportCreateSamplesDelta(prev, current, null()) };
        if delta.is_null() {
            self.fallback_power_estimate();
            return;
        }

        let mut cpu_energy: f64 = 0.0;
        let mut gpu_energy: f64 = 0.0;
        let mut ane_energy: f64 = 0.0;

        if let Some(iter) = IOReportIterator::new(delta) {
            for channel in iter {
                let joules = energy_to_joules(channel.value, &channel.unit);
                let name = channel.name.to_lowercase();

                if name.contains("cpu") && !name.contains("gpu") {
                    cpu_energy += joules;
                } else if name.contains("gpu") {
                    gpu_energy += joules;
                } else if name.contains("ane") {
                    ane_energy += joules;
                }
            }
        }

        let seconds = elapsed.as_secs_f64().max(0.001);
        self.cpu_power = (cpu_energy / seconds) as f32;
        self.gpu_power = (gpu_energy / seconds) as f32;
        self.ane_power = (ane_energy / seconds) as f32;
        self.total_system_power = self.cpu_power + self.gpu_power + self.ane_power;
    }

    fn fallback_power_estimate(&mut self) {
        use sysinfo::System;

        let mut sys = System::new_all();
        sys.refresh_all();
        std::thread::sleep(Duration::from_millis(50));
        sys.refresh_all();

        let cpu_usage: f32 =
            sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32;

        let base_power = 2.0;
        let max_cpu_power = 15.0;
        self.cpu_power = base_power + (cpu_usage / 100.0) * max_cpu_power;
        self.gpu_power = 1.0;
        self.ane_power = 0.0;
        self.total_system_power = self.cpu_power + self.gpu_power;
    }

    fn refresh_power_mode(&mut self) {
        if let Ok(output) = Command::new("pmset").args(["-g"]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);

            if stdout.contains("lowpowermode 1") {
                self.power_mode = PowerMode::LowPower;
            } else if stdout.contains("highpowermode 1") {
                self.power_mode = PowerMode::HighPerformance;
            } else {
                self.power_mode = PowerMode::Automatic;
            }
        }
    }

    pub fn cpu_power_watts(&self) -> f32 {
        self.cpu_power
    }

    pub fn gpu_power_watts(&self) -> f32 {
        self.gpu_power
    }

    pub fn total_power_watts(&self) -> f32 {
        self.total_system_power
    }

    pub fn power_mode(&self) -> PowerMode {
        self.power_mode
    }

    pub fn power_mode_label(&self) -> &'static str {
        match self.power_mode {
            PowerMode::LowPower => "Low Power",
            PowerMode::Automatic => "Automatic",
            PowerMode::HighPerformance => "High Performance",
            PowerMode::Unknown => "Unknown",
        }
    }
}

impl Drop for PowerData {
    fn drop(&mut self) {
        if let Some(sample) = self.last_sample {
            unsafe {
                CFRelease(sample as _);
            }
        }
    }
}

fn energy_to_joules(value: i64, unit: &str) -> f64 {
    let val = value as f64;
    match unit {
        "mJ" => val / 1_000.0,
        "uJ" => val / 1_000_000.0,
        "nJ" => val / 1_000_000_000.0,
        _ => val / 1_000_000.0,
    }
}
