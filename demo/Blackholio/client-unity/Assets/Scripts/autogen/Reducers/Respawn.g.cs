// THIS FILE IS AUTOMATICALLY GENERATED BY SPACETIMEDB. EDITS TO THIS FILE
// WILL NOT BE SAVED. MODIFY TABLES IN YOUR MODULE SOURCE CODE INSTEAD.

// This was generated using spacetimedb cli version 1.2.0 (commit ).

#nullable enable

using System;
using SpacetimeDB.ClientApi;
using System.Collections.Generic;
using System.Runtime.Serialization;

namespace SpacetimeDB.Types
{
    public sealed partial class RemoteReducers : RemoteBase
    {
        public delegate void RespawnHandler(ReducerEventContext ctx);
        public event RespawnHandler? OnRespawn;

        public void Respawn()
        {
            conn.InternalCallReducer(new Reducer.Respawn(), this.SetCallReducerFlags.RespawnFlags);
        }

        public bool InvokeRespawn(ReducerEventContext ctx, Reducer.Respawn args)
        {
            if (OnRespawn == null)
            {
                if (InternalOnUnhandledReducerError != null)
                {
                    switch (ctx.Event.Status)
                    {
                        case Status.Failed(var reason): InternalOnUnhandledReducerError(ctx, new Exception(reason)); break;
                        case Status.OutOfEnergy(var _): InternalOnUnhandledReducerError(ctx, new Exception("out of energy")); break;
                    }
                }
                return false;
            }
            OnRespawn(
                ctx
            );
            return true;
        }
    }

    public abstract partial class Reducer
    {
        [SpacetimeDB.Type]
        [DataContract]
        public sealed partial class Respawn : Reducer, IReducerArgs
        {
            string IReducerArgs.ReducerName => "respawn";
        }
    }

    public sealed partial class SetReducerFlags
    {
        internal CallReducerFlags RespawnFlags;
        public void Respawn(CallReducerFlags flags) => RespawnFlags = flags;
    }
}
