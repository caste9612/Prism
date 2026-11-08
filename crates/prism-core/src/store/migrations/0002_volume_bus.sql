-- Schema version 2 (T2): bus of the disk that holds the volume (usb, sata, nvme, virtual...).
-- Windows reports external hard disks as "fixed": the bus tells internal and external apart
-- (F01), and SMART is often unavailable behind USB adapters (F17-AC4).
ALTER TABLE volumes ADD COLUMN bus_type TEXT;
