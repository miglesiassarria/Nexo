# 0022 · Diseño

El estado desactivado se representa explícitamente por vía y aplicación, manteniendo el límite configurado para poder restaurarlo. La ausencia de configuración sigue rechazando las vías de suscripción. La base de datos conserva los límites existentes y la interfaz permite alternar el estado. Las modificaciones de permisos no deben restablecerlo.

El adaptador de Gemini identifica el 429 remoto y comunica detalles seguros reconocidos sin registrar cuerpos de respuesta; los errores locales y remotos mantienen sus códigos diferenciados. Se verifican el aislamiento entre vías, la persistencia, la migración, la respuesta al cliente y la interfaz. La causa de los rechazos históricos no se puede deducir de estadísticas que no conservan el cuerpo.

Se sustituye la obligación absoluta del ADR 0001 mediante un ADR nuevo: se mantienen los límites por defecto y el usuario puede desactivarlos explícitamente. Se descarta interpretar ausencia de límite como desactivación, porque una configuración incompleta quedaría abierta sin intención.
