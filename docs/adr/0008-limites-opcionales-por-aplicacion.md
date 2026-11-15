# ADR 0008: Desactivación explícita del límite por aplicación

- **Fecha:** 2026-11-15
- **Estado:** aceptada por el usuario mediante la spec 0022

## Decisión

Los límites de Nexo para vías de suscripción siguen activos por defecto y cada aplicación y vía conserva su configuración anterior. El usuario puede desactivarlos y reactivarlos de forma explícita. Una configuración ausente o inválida no equivale a una desactivación. La interfaz explica que esta opción no afecta a los límites del proveedor.

Esta decisión sustituye la obligación absoluta de límites de la sección de multiplexación del ADR 0001. Permitir tráfico sin protección local puede agotar antes la cuota compartida o provocar restricciones del proveedor; es una decisión del usuario para cada aplicación y vía.
