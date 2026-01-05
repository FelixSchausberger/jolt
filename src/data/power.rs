use color_eyre::eyre::Result;
use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::dictionary::{CFDictionaryRef, CFMutableDictionaryRef};
use core_foundation::string::CFString;
use core_foundation_sys::base::kCFAllocatorDefault;
use core_foundation_sys::dictionary::{CFDictionaryCreateMutableCopy, CFDictionaryGetCount};
use std::ffi::c_void;
use std::process::Command;
use std::ptr;
use std::time::{Duration, Instant};

type IOReportSubscriptionRef = *const c_void;

#[link(name = "IOReport", kind = "dylib")]
extern "C" {
    fn IOReportCopyChannelsInGroup(
        group: core_foundation_sys::string::CFStringRef,
        subgroup: core_foundation_sys::string::CFStringRef,
        a: u64,
        b: u64,
        c: u64,
    ) -> CFDictionaryRef;

    fn IOReportCreateSubscription(
        a: CFTypeRef,
        b: CFMutableDictionaryRef,
        c: *mut CFMutableDictionaryRef,
        d: u64,
        e: CFTypeRef,
    ) -> IOReportSubscriptionRef;

    fn IOReportCreateSamples(
        subscription: IOReportSubscriptionRef,
        channels: CFMutableDictionaryRef,
        nil: CFTypeRef,
    ) -> CFDictionaryRef;

    fn IOReportCreateSamplesDelta(
        prev: CFDictionaryRef,
        current: CFDictionaryRef,
        nil: CFTypeRef,
    ) -> CFDictionaryRef;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    LowPower,
    Automatic,
    HighPerformance,
    Unknown,
}

struct IOReportSubscription {
    subscription: IOReportSubscriptionRef,
    channels: CFMutableDictionaryRef,
}

impl IOReportSubscription {
    fn new() -> Option<Self> {
        unsafe {
            let energy_group = CFString::new("Energy Model");

            let energy_channels = IOReportCopyChannelsInGroup(
                energy_group.as_concrete_TypeRef(),
                ptr::null(),
                0,
                0,
                0,
            );

            if energy_channels.is_null() {
                return None;
            }

            let count = CFDictionaryGetCount(energy_channels);
            let channels =
                CFDictionaryCreateMutableCopy(kCFAllocatorDefault, count, energy_channels);

            CFRelease(energy_channels as CFTypeRef);

            if channels.is_null() {
                return None;
            }

            let mut sub_channels: CFMutableDictionaryRef = ptr::null_mut();
            let subscription = IOReportCreateSubscription(
                ptr::null(),
                channels,
                &mut sub_channels,
                0,
                ptr::null(),
            );

            if subscription.is_null() {
                CFRelease(channels as CFTypeRef);
                return None;
            }

            Some(Self {
                subscription,
                channels,
            })
        }
    }

    fn sample(&self) -> Option<CFDictionaryRef> {
        unsafe {
            let sample = IOReportCreateSamples(self.subscription, self.channels, ptr::null());
            if sample.is_null() {
                None
            } else {
                Some(sample)
            }
        }
    }
}

impl Drop for IOReportSubscription {
    fn drop(&mut self) {
        unsafe {
            if !self.channels.is_null() {
                CFRelease(self.channels as CFTypeRef);
            }
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
        if let Some(ref subscription) = self.subscription {
            if let Some(current_sample) = subscription.sample() {
                if let (Some(prev_sample), Some(prev_time)) =
                    (self.last_sample, self.last_sample_time)
                {
                    let elapsed = prev_time.elapsed();
                    self.calculate_power_from_samples(prev_sample, current_sample, elapsed);

                    unsafe {
                        CFRelease(prev_sample as CFTypeRef);
                    }
                }

                self.last_sample = Some(current_sample);
                self.last_sample_time = Some(Instant::now());
                return;
            }
        }

        self.fallback_power_estimate();
    }

    fn calculate_power_from_samples(
        &mut self,
        prev: CFDictionaryRef,
        current: CFDictionaryRef,
        elapsed: Duration,
    ) {
        unsafe {
            let delta = IOReportCreateSamplesDelta(prev, current, ptr::null());
            if delta.is_null() {
                self.fallback_power_estimate();
                return;
            }

            let (cpu, gpu, ane) = Self::parse_energy_delta(delta, elapsed);
            CFRelease(delta as CFTypeRef);

            self.cpu_power = cpu;
            self.gpu_power = gpu;
            self.ane_power = ane;
            self.total_system_power = cpu + gpu + ane;
        }
    }

    fn parse_energy_delta(delta: CFDictionaryRef, elapsed: Duration) -> (f32, f32, f32) {
        use core_foundation::array::CFArray;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::number::CFNumber;

        let mut cpu_energy: f64 = 0.0;
        let mut gpu_energy: f64 = 0.0;
        let mut ane_energy: f64 = 0.0;

        let delta_dict: CFDictionary<CFString, CFArray> =
            unsafe { CFDictionary::wrap_under_get_rule(delta) };

        let channels_key = CFString::new("IOReportChannels");
        if let Some(channels) = delta_dict.find(&channels_key) {
            let channels_array: &CFArray = unsafe { std::mem::transmute(channels) };

            for i in 0..channels_array.len() {
                if let Some(channel) = channels_array.get(i) {
                    let channel_dict: &CFDictionary<CFString, CFString> =
                        unsafe { std::mem::transmute(&channel) };

                    let channel_name = channel_dict
                        .find(&CFString::new("IOReportChannelName"))
                        .map(|v| unsafe {
                            let s: &CFString = std::mem::transmute(v);
                            s.to_string()
                        })
                        .unwrap_or_default();

                    let value = channel_dict
                        .find(&CFString::new("IOReportSimpleValue"))
                        .and_then(|v| unsafe {
                            let n: &CFNumber = std::mem::transmute(v);
                            n.to_i64()
                        })
                        .unwrap_or(0) as f64;

                    let unit = channel_dict
                        .find(&CFString::new("IOReportUnit"))
                        .map(|v| unsafe {
                            let s: &CFString = std::mem::transmute(v);
                            s.to_string()
                        })
                        .unwrap_or_default();

                    let joules = match unit.as_str() {
                        "mJ" => value / 1000.0,
                        "uJ" => value / 1_000_000.0,
                        "nJ" => value / 1_000_000_000.0,
                        _ => value / 1_000_000.0,
                    };

                    let name_lower = channel_name.to_lowercase();
                    if name_lower.contains("cpu") {
                        cpu_energy += joules;
                    } else if name_lower.contains("gpu") {
                        gpu_energy += joules;
                    } else if name_lower.contains("ane") {
                        ane_energy += joules;
                    }
                }
            }
        }

        let seconds = elapsed.as_secs_f64().max(0.001);
        let cpu_watts = (cpu_energy / seconds) as f32;
        let gpu_watts = (gpu_energy / seconds) as f32;
        let ane_watts = (ane_energy / seconds) as f32;

        (cpu_watts, gpu_watts, ane_watts)
    }

    fn fallback_power_estimate(&mut self) {
        use sysinfo::System;

        let mut sys = System::new_all();
        sys.refresh_all();
        std::thread::sleep(std::time::Duration::from_millis(50));
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
                CFRelease(sample as CFTypeRef);
            }
        }
    }
}
