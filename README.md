
|Alumno                   | Padrón |
|-------------------------|--------|
|Edgargo Francisco Saez   | 104896 |
|Tomás Vainstein Aranguren| 109043 |
|Fabio Sicca              | 104892 |

# Proyecto de Ride Sharing Distribuido

Este repositorio contiene la implementación de un sistema distribuido de ride sharing desarrollado en Rust. El sistema incluye funcionalidades de coordinación entre conductores, pasajeros y un servicio de pagos. Utiliza el modelo de actores de Actix para manejar concurrencia y comunicaciones asíncronas.

---

## Características Principales

- **Arquitectura Distribuida:** Los componentes del sistema están diseñados para funcionar en entornos distribuidos y ser resilientes a fallas.
- **Sistema de Elección de Líder (Algoritmo de Anillo):** Implementa un algoritmo eficiente para manejar fallas en los nodos líderes.
- **Comunicación Asíncrona:** Basado en `actix` y `tokio`, utiliza tareas asíncronas para operaciones concurrentes.
- **Gestor de Viajes:** Asigna conductores a pasajeros y actualiza posiciones en tiempo real durante un viaje.
- **Reconexión Automática:** Los nodos intentan reconectarse de manera automática en caso de desconexión.
- **Sistema de Pagos:** Integra un servicio externo para procesar pagos.

---

## Requisitos

Para ejecutar este proyecto necesitas:

- **Rust** (versión estable reciente)
- **Cargo**
- **Tokio** y **Actix** (ya incluidos como dependencias en el proyecto)

---

## Instalación

1. Clona este repositorio:

   ```bash
   git clone https://github.com/usuario/proyecto-ride-sharing.git
   cd proyecto-ride-sharing
   ```

2. Instala las dependencias:

   ```bash
   cargo build
   ```

3. Configura los puertos y otros parámetros en el archivo `config.txt`.

---

## Uso

### Ejecutar un conductor

Puedes iniciar un nodo conductor con el siguiente comando:

```bash
cargo run 6001 3 3
```

Donde:

- `6001` es el puerto del conductor.
- `3 3` son las coordenadas iniciales del conductor.

### Logs Verbosos

Para habilitar logs detallados (sistema de PINGS y ubicaciones de los coches en movimiento), utiliza la variable de entorno `RUST_LOG`:

```bash
RUST_LOG=info cargo run 6001 3 3 
```

---

## Documentación del Código

Para generar la documentación:

```bash
cargo doc --open
```

---

## Funcionalidades

### Elección de Líder

El sistema utiliza un algoritmo de elección de líder basado en anillos. En caso de que el líder actual falle, los nodos restantes detectan la falla y eligen un nuevo líder de manera automática.

### Sistema de Pings

Cada nodo envía pings periódicos para verificar la disponibilidad de los demás nodos. Si un nodo no responde dentro del intervalo de tiempo definido, se considera como desconectado.

### Reconexión Automática

Cuando un nodo desconectado vuelve a estar disponible, intenta reconectarse automáticamente al sistema.

### Viajes

1. **Inicio de Viaje:** Un pasajero solicita un viaje y el sistema asigna un conductor.
2. **Actualización en Tiempo Real:** Durante el viaje, el sistema actualiza la posición del conductor.
3. **Finalización:** Cuando se llega al destino, se registra la finalización del viaje y se procesa el pago.

---

## Arquitectura del Código

El proyecto está dividido en los siguientes módulos:

1. **driver:** Maneja la lógica del conductor, incluyendo actualización de posiciones y pings.
2. **models:** Define las estructuras de datos principales como `RideRequest`, `PositionUpdate` y `DriverStatus`.
3. **utils:** Contiene funciones auxiliares como la lectura de configuración.
4. **ride\_manager:** Gestiona los viajes y las asignaciones de pasajeros a conductores.

---

## Licencia

Este proyecto está licenciado bajo la [MIT License](LICENSE).


